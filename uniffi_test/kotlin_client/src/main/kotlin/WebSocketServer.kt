import org.java_websocket.WebSocket
import org.java_websocket.handshake.ClientHandshake
import org.java_websocket.server.WebSocketServer as JWebSocketServer
import uniffi.uniffi_test.*
import java.net.InetSocketAddress
import java.nio.ByteBuffer

/**
 * WebSocket server transport for ractor-wormhole.
 * Uses Java-WebSocket library.
 */
class ChatWebSocketServer(
    port: Int,
    private val runtime: WormholeRuntime,
    private val onClientConnected: (WormholeConnection, ServerClientTransport) -> Unit
) : JWebSocketServer(InetSocketAddress(port)) {

    private val clientTransports = mutableMapOf<WebSocket, ServerClientTransport>()

    override fun onOpen(conn: WebSocket, handshake: ClientHandshake) {
        println("[Server] Client connected from ${conn.remoteSocketAddress}")

        // Create transport for this client
        val transport = ServerClientTransport(conn)
        clientTransports[conn] = transport

        // Create wormhole connection
        val identifier = "server-to-${conn.remoteSocketAddress}"
        try {
            val wormholeConn = runtime.createConnection(identifier, transport)
            transport.setWormholeConnection(wormholeConn)
            onClientConnected(wormholeConn, transport)
        } catch (e: Exception) {
            println("[Server] Failed to create connection: ${e.message}")
            conn.close(1011, "Failed to create wormhole connection")
        }
    }

    override fun onClose(conn: WebSocket, code: Int, reason: String, remote: Boolean) {
        println("[Server] Client disconnected: $code $reason")
        clientTransports.remove(conn)?.let { transport ->
            transport.wormholeConnection?.onMessageReceived(FfiConduitMessage.Close(reason.ifEmpty { null }))
        }
    }

    override fun onMessage(conn: WebSocket, message: String) {
        println("[Server] Received text from client: ${message.take(100)}...")
        clientTransports[conn]?.wormholeConnection?.onMessageReceived(
            FfiConduitMessage.Text(message)
        )
    }

    override fun onMessage(conn: WebSocket, message: ByteBuffer) {
        println("[Server] Received binary from client: ${message.remaining()} bytes")
        val bytes = ByteArray(message.remaining())
        message.get(bytes)
        clientTransports[conn]?.wormholeConnection?.onMessageReceived(
            FfiConduitMessage.Binary(bytes)
        )
    }

    override fun onError(conn: WebSocket?, ex: Exception) {
        println("[Server] Error: ${ex.message}")
        ex.printStackTrace()
    }

    override fun onStart() {
        println("[Server] WebSocket server started on port $port")
    }
}

/**
 * Transport for a single client connection on the server side.
 */
class ServerClientTransport(
    private val webSocket: WebSocket
) : TransportCallback {

    var wormholeConnection: WormholeConnection? = null
        private set

    fun setWormholeConnection(conn: WormholeConnection) {
        this.wormholeConnection = conn
    }

    override fun sendMessage(message: FfiConduitMessage) {
        when (message) {
            is FfiConduitMessage.Text -> {
                println("[ServerTransport] Sending text: ${message.content.take(100)}...")
                webSocket.send(message.content)
            }
            is FfiConduitMessage.Binary -> {
                println("[ServerTransport] Sending binary: ${message.data.size} bytes")
                webSocket.send(message.data)
            }
            is FfiConduitMessage.Close -> {
                println("[ServerTransport] Closing: ${message.reason}")
                webSocket.close(1000, message.reason ?: "")
            }
        }
    }

    override fun close() {
        println("[ServerTransport] Close requested")
        webSocket.close(1000, "Server closing")
    }
}
