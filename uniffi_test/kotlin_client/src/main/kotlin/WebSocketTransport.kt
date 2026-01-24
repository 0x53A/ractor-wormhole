import okhttp3.*
import okio.ByteString
import uniffi.uniffi_test.*
import java.util.concurrent.TimeUnit

/**
 * WebSocket-based transport implementation for ractor-wormhole.
 * This bridges the UniFFI callback interface with OkHttp WebSocket.
 */
class WebSocketTransport(
    private val url: String,
    private val onConnected: (WebSocketTransport) -> Unit,
    private val onDisconnected: (String?) -> Unit,
    private val onError: (Throwable) -> Unit
) : TransportCallback {

    private val client = OkHttpClient.Builder()
        .readTimeout(0, TimeUnit.MILLISECONDS)  // No timeout for WebSocket
        .build()

    private var webSocket: WebSocket? = null
    private var connection: WormholeConnection? = null

    /**
     * Set the connection reference so we can forward incoming messages to Rust.
     */
    fun setConnection(conn: WormholeConnection) {
        this.connection = conn
    }

    /**
     * Connect to the WebSocket server.
     */
    fun connect() {
        val request = Request.Builder()
            .url(url)
            .build()

        webSocket = client.newWebSocket(request, object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) {
                println("[WebSocket] Connected to $url")
                onConnected(this@WebSocketTransport)
            }

            override fun onMessage(webSocket: WebSocket, text: String) {
                println("[WebSocket] Received text: ${text.take(100)}...")
                connection?.onMessageReceived(FfiConduitMessage.Text(text))
            }

            override fun onMessage(webSocket: WebSocket, bytes: ByteString) {
                println("[WebSocket] Received binary: ${bytes.size} bytes")
                connection?.onMessageReceived(FfiConduitMessage.Binary(bytes.toByteArray()))
            }

            override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
                println("[WebSocket] Closing: $code $reason")
                webSocket.close(code, reason)
            }

            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                println("[WebSocket] Closed: $code $reason")
                connection?.onMessageReceived(FfiConduitMessage.Close(reason.ifEmpty { null }))
                onDisconnected(reason.ifEmpty { null })
            }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                println("[WebSocket] Error: ${t.message}")
                onError(t)
            }
        })
    }

    // TransportCallback implementation - called by Rust to send data
    override fun sendMessage(message: FfiConduitMessage) {
        when (message) {
            is FfiConduitMessage.Text -> {
                println("[Transport] Sending text: ${message.content.take(100)}...")
                webSocket?.send(message.content)
            }
            is FfiConduitMessage.Binary -> {
                println("[Transport] Sending binary: ${message.data.size} bytes")
                webSocket?.send(ByteString.of(*message.data))
            }
            is FfiConduitMessage.Close -> {
                println("[Transport] Closing: ${message.reason}")
                webSocket?.close(1000, message.reason ?: "")
            }
        }
    }

    override fun close() {
        println("[Transport] Close requested")
        webSocket?.close(1000, "Client closing")
        client.dispatcher.executorService.shutdown()
    }
}
