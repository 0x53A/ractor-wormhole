import uniffi.uniffi_test.*
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.Scanner
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicInteger

/**
 * Chat application demonstrating typed ActorRef and RpcReplyPort FFI bindings.
 *
 * New architecture:
 * - Handler traits with single `receive` method (uniform pattern)
 * - FFI message enums mirror Rust enums
 * - `to_ref()` converts local actors to ActorRefs
 * - Same ActorRef type for local and remote refs
 */
fun main(args: Array<String>) {
    val mode = args.getOrElse(0) { "help" }

    when (mode) {
        "server" -> {
            val port = args.getOrElse(1) { "8080" }.toInt()
            runServer(port)
        }
        "client" -> {
            val serverUrl = args.getOrElse(1) { "ws://127.0.0.1:8080" }
            val name = args.getOrElse(2) { "Anonymous" }
            runClient(serverUrl, name)
        }
        else -> {
            println("=== Kotlin Wormhole Chat (Typed API Demo) ===")
            println()
            println("Architecture:")
            println("  - Handler.receive(msg) - uniform message handling")
            println("  - FfiChatClientMessage enum - typed messages")
            println("  - local.to_ref() - convert to sendable ref")
            println("  - ref.send(msg) - uniform send interface")
            println()
            println("Usage:")
            println("  server [port]              - Start chat server (default: 8080)")
            println("  client [url] [name]        - Connect to server")
            println()
            println("Examples:")
            println("  ./gradlew run --args=\"server 8080\"")
            println("  ./gradlew run --args=\"client ws://127.0.0.1:8080 Alice\"")
        }
    }
}

// =============================================================================
// Server Implementation
// =============================================================================

fun runServer(port: Int) {
    println("=== Chat Server (Typed API Demo) ===")
    println("Starting server on port $port...")
    println()

    val runtime = WormholeRuntime()
    println("[Server] Runtime created")

    // Track connected clients by their assigned alias
    val clients = ConcurrentHashMap<String, FfiChatClientActorRef>()
    val userCounter = AtomicInteger(0)

    // Create the ChatServer actor - handles messages from clients
    // Uses the new ChatServerHandler interface
    val chatServerHandler = object : ChatServerHandler {
        override fun receivePostMessage(content: String, replyPort: FfiAckReplyPort) {
            println("[Server] Broadcasting message: $content")

            // Broadcast to all connected clients using the uniform send() method
            clients.forEach { (alias, clientRef) ->
                try {
                    // Can use either send(FfiChatClientMessage) or convenience methods
                    clientRef.send(FfiChatClientMessage.MessageReceived("Server", content))
                } catch (e: Exception) {
                    println("[Server] Failed to send to $alias: ${e.message}")
                }
            }

            // Acknowledge receipt
            try {
                replyPort.ack()
            } catch (e: Exception) {
                println("[Server] Failed to ack: ${e.message}")
            }
        }

        override fun receiveListUsers(replyPort: FfiUserListReplyPort) {
            val userList = clients.keys.toList()
            println("[Server] User list requested: $userList")
            try {
                replyPort.reply(userList)
            } catch (e: Exception) {
                println("[Server] Failed to send user list: ${e.message}")
            }
        }
    }

    val chatServerActor = runtime.createChatServerActor(chatServerHandler)
    println("[Server] ChatServer actor created")

    // Create the Hub actor - handles client connections
    // Uses the new HubHandler interface
    val hubHandler = object : HubHandler {
        override fun receiveConnect(clientActor: FfiChatClientActorRef, replyPort: FfiConnectReplyPort) {
            // Assign a user alias
            val alias = "User${userCounter.incrementAndGet()}"
            println("[Server] Client connecting, assigned alias: $alias")

            // Store the client's actor ref so we can send them notifications
            clients[alias] = clientActor

            // Notify other clients about the new user using the uniform send()
            clients.forEach { (otherAlias, otherClient) ->
                if (otherAlias != alias) {
                    try {
                        otherClient.send(FfiChatClientMessage.UserConnected(alias))
                    } catch (e: Exception) {
                        println("[Server] Failed to notify $otherAlias: ${e.message}")
                    }
                }
            }

            // Reply with the user's alias and the chat server actor
            // Use to_ref() to get a sendable reference to our local actor
            try {
                replyPort.reply(alias, chatServerActor)
                println("[Server] Sent connect reply to $alias")
            } catch (e: Exception) {
                println("[Server] Failed to reply: ${e.message}")
                clients.remove(alias)
            }
        }
    }

    val hubActor = runtime.createHubActor(hubHandler)
    println("[Server] Hub actor created")

    // Create WebSocket server
    val server = ChatWebSocketServer(port, runtime) { connection, transport ->
        println("[Server] New WebSocket connection established")

        Thread {
            try {
                connection.waitForHandshake(5000u)
                println("[Server] Handshake complete")

                connection.publishHubActor("hub", hubActor)
                println("[Server] Published 'hub' actor")

            } catch (e: Exception) {
                println("[Server] Error during setup: ${e.message}")
            }
        }.start()
    }

    server.start()
    println("[Server] Server started. Waiting for connections...")
    println("[Server] Press Ctrl+C to stop.")
    println()

    // Read server console input and broadcast to all clients
    val scanner = Scanner(System.`in`)
    print("> ")
    while (scanner.hasNextLine()) {
        val message = scanner.nextLine()
        if (message.isNotBlank()) {
            clients.forEach { (alias, clientRef) ->
                try {
                    // Use the uniform send() with enum
                    clientRef.send(FfiChatClientMessage.MessageReceived("Server", message))
                } catch (e: Exception) {
                    println("[Server] Failed to send to $alias: ${e.message}")
                }
            }
        }
        print("> ")
    }

    server.stop()
    hubActor.stop()
    chatServerActor.stop()
    runtime.shutdown()
}

// =============================================================================
// Client Implementation
// =============================================================================

fun runClient(serverUrl: String, name: String) {
    println("=== Chat Client (Typed API Demo) ===")
    println("Connecting to: $serverUrl")
    println("Your name: $name")
    println()

    val runtime = WormholeRuntime()
    println("[Client] Runtime created")

    // Create our ChatClient actor using the new ChatClientHandler
    // Single receive() method handles all message types via enum
    val clientHandler = object : ChatClientHandler {
        override fun receive(msg: FfiChatClientMessage) {
            // Pattern match on the FFI message enum
            when (msg) {
                is FfiChatClientMessage.UserConnected -> {
                    println()
                    println("[*] ${msg.alias} joined the chat")
                    print("> ")
                }
                is FfiChatClientMessage.UserDisconnected -> {
                    println()
                    println("[*] ${msg.alias} left the chat")
                    print("> ")
                }
                is FfiChatClientMessage.MessageReceived -> {
                    println()
                    println("[${msg.sender}]: ${msg.content}")
                    print("> ")
                }
                is FfiChatClientMessage.ServerShutdown -> {
                    println()
                    println("[*] Server is shutting down")
                }
            }
        }
    }

    val clientActor = runtime.createChatClientActor(clientHandler)
    println("[Client] ChatClient actor created")

    val connectedLatch = CountDownLatch(1)
    val disconnectedLatch = CountDownLatch(1)

    var connection: WormholeConnection? = null
    var serverActor: FfiChatServerActorRef? = null
    var myAlias: String? = null

    val transport = WebSocketTransport(
        url = serverUrl,
        onConnected = { tr ->
            println("[Client] WebSocket connected")
            try {
                connection = runtime.createConnection("client-connection", tr)
                tr.setConnection(connection!!)
                connectedLatch.countDown()
            } catch (e: Exception) {
                println("[Client] Failed to create connection: ${e.message}")
            }
        },
        onDisconnected = { reason ->
            println("[Client] Disconnected: $reason")
            disconnectedLatch.countDown()
        },
        onError = { error ->
            println("[Client] Error: ${error.message}")
            disconnectedLatch.countDown()
        }
    )

    println("[Client] Connecting to WebSocket...")
    transport.connect()

    if (!connectedLatch.await(10, TimeUnit.SECONDS)) {
        println("[Client] Connection timeout!")
        runtime.shutdown()
        return
    }

    val conn = connection ?: run {
        println("[Client] Connection is null!")
        runtime.shutdown()
        return
    }

    try {
        println("[Client] Waiting for handshake...")
        conn.waitForHandshake(5000u)
        println("[Client] Handshake complete!")

        println("[Client] Looking for 'hub' actor...")
        val hubActor = conn.getRemoteHubActor("hub")
        println("[Client] Found Hub actor!")

        // Connect to the hub - sends our actor ref to the server
        println("[Client] Connecting to hub...")
        val result = hubActor.connect(clientActor)

        myAlias = result.userAlias
        serverActor = result.serverActor

        println("[Client] Connected! Server assigned alias: $myAlias")
        println()
        println("You are now connected as '$myAlias'")
        println("Commands: 'users' to list users, 'quit' to exit")
        println()

        val scanner = Scanner(System.`in`)
        print("> ")
        while (scanner.hasNextLine() && !disconnectedLatch.await(0, TimeUnit.MILLISECONDS)) {
            val message = scanner.nextLine()

            if (message.equals("quit", ignoreCase = true)) {
                break
            }

            if (message.equals("users", ignoreCase = true)) {
                try {
                    val users = serverActor!!.listUsers()
                    println("[*] Connected users: ${users.joinToString(", ")}")
                } catch (e: Exception) {
                    println("[Client] Failed to get user list: ${e.message}")
                }
                print("> ")
                continue
            }

            if (message.isNotBlank()) {
                try {
                    serverActor!!.postMessage(message)
                } catch (e: Exception) {
                    println("[Client] Failed to send: ${e.message}")
                }
            }
            print("> ")
        }

    } catch (e: WormholeException) {
        println("[Client] Wormhole error: ${e.message}")
    } catch (e: Exception) {
        println("[Client] Error: ${e.message}")
        e.printStackTrace()
    } finally {
        println("[Client] Disconnecting...")
        conn.disconnect()
        clientActor.stop()
        runtime.shutdown()
        println("[Client] Done!")
    }
}
