# SSH Transport for Ractor Wormhole

This module provides composable SSH tunneling primitives for Ractor Wormhole, allowing you to connect to remote Unix sockets or TCP services through SSH.

## Architecture

The SSH transport is built with **composable primitives** that can be combined:

1. **Low-level**: `connect_ssh()` - Establishes SSH connection, returns Handle
2. **Mid-level**: `ssh_unix_socket_channel()`, `ssh_tcp_channel()` - Returns ChannelStream (AsyncRead + AsyncWrite)
3. **Adapter**: `channel_to_conduit()` - Converts any AsyncRead/AsyncWrite to (ConduitSource, ConduitSink)
4. **High-level**: `connect_via_ssh_to_unix_socket()`, `connect_via_ssh_to_tcp()` - Complete Portal setup

This design allows you to:
- Compose transports (e.g., WebSocket over TCP over SSH)
- Reuse SSH connections for multiple channels
- Mix and match primitives as needed

## Features

- **Unix Socket Tunneling**: Connect to Unix sockets on remote machines via SSH
- **TCP Port Tunneling**: Connect to TCP services on remote machines via SSH
- **Multiple Authentication Methods**:
  - Password authentication
  - Public key authentication (with optional passphrase)
  - Agent authentication (planned)
- **Pure Rust**: Uses `russh` (no C dependencies)

## Dependencies

The SSH transport uses `russh`, a pure Rust SSH implementation. Add it to your dependencies:

```toml
[dependencies]
ractor_wormhole = { version = "0.1.0", features = ["ssh_client"] }
```

## Usage

### High-Level: Complete Connection Setup

The simplest way - connects and registers with Nexus in one call:

```rust
use ractor_wormhole::conduit::ssh::client::{
    connect_via_ssh_to_unix_socket, SshAuthMethod, SshConfig
};
use ractor_wormhole::nexus::start_nexus;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Start the nexus
    let nexus = start_nexus(None, None).await?;

    // Configure SSH connection
    let ssh_config = SshConfig {
        host: "example.com".to_string(),
        port: 22,
        username: "myuser".to_string(),
        auth_method: SshAuthMethod::Password("mypassword".to_string()),
    };

    // Connect via SSH to remote Unix socket
    let portal = connect_via_ssh_to_unix_socket(
        nexus,
        ssh_config,
        "/tmp/my_socket"
    ).await?;

    // Use the portal normally...
    Ok(())
}
```

### Using Public Key Authentication

```rust
let ssh_config = SshConfig {
    host: "example.com".to_string(),
    port: 22,
    username: "myuser".to_string(),
    auth_method: SshAuthMethod::PublicKey {
        private_key_path: "/home/user/.ssh/id_rsa".to_string(),
        passphrase: Some("key_passphrase".to_string()),
   

### Composable Primitives

For more control, use the lower-level primitives:

```rust
use ractor_wormhole::conduit::ssh::client::{
    connect_ssh, ssh_unix_socket_channel, ssh_tcp_channel, channel_to_conduit
};

// 1. Establish SSH connection (reusable)
let mut ssh_handle = connect_ssh(ssh_config).await?;

// 2. Open channels as needed
let unix_channel = ssh_unix_socket_channel(&mut ssh_handle, "/tmp/my_socket").await?;
let tcp_channel = ssh_tcp_channel(&mut ssh_handle, "localhost", 8080).await?;

// 3. Convert to conduit primitives
let (unix_source, unix_sink) = channel_to_conduit(unix_channel);
let (tcp_source, tcp_sink) = channel_to_conduit(tcp_channel);

// 4. Register with nexus manually
nexus.ask(|rpc| NexusActorMessage::Connected("my_id".to_string(), unix_sink, rpc), None).await?;

// 5. Start receive loop
ractor::concurrency::spawn(async move {
    conduit::receive_loop(unix_source, "my_id".to_string(), portal).await
});
```

This composability means you could even wrap a WebSocket over TCP-over-SSH! },
};
```

### Connecting to a Remote TCP Port

```rust
use ractor_wormhole::conduit::ssh::client::connect_via_ssh_to_tcp;

let portal = connect_via_ssh_to_tcp(
    nexus,
    ssh_config,
    "localhost".to_string(),  // Remote host (from SSH server's perspective)
    8080,                      // Remote port
).await?;
```

## Sample Application

The sample app includes SSH support. To use it:

### Server Side (Remote Machine)

First, start a server on the remote machine using Unix sockets:
using `russh` (pure Rust, no C dependencies)
2. For Unix sockets: 
   - First tries OpenSSH's `direct-streamlocal@openssh.com` extension
   - Falls back to executing `socat STDIO UNIX-CONNECT:/path` if not available
3. For TCP ports: Using SSH's standard `direct-tcpip` channel forwarding
4. Converting the russh Channel into a ChannelStream (AsyncRead + AsyncWrite)
5. Creating Conduit sink/source pairs through the `channel_to_conduit` adapter
6. Registering the connection with the Nexus as a Portal
 (Fallback Method)

If the remote OpenSSH server doesn't support `direct-streamlocal@openssh.com`, the code automatically falls back to using `socat`. In this case, the remote machine should have `socat` installed:

```bash
# Debian/Ubuntu
sudo apt-get install socat

# RHEL/CentOS
sudo yum install socat

# macOS
brew install socat
```

**Note**: Modern OpenSSH servers (7.0+) support direct-streamlocal, so socat is usually not needed.go run --features ssh -- client \
    --ssh "user@remote.example.com:22/tmp/ractor_wormhole_sample_app" \
    --ssh-password "mypassword"

# Using public key authentication
cargo run --features ssh -- client \
    --ssh "user@remote.examplaccepts all SSH host keys without verification (`ServerCheckMethod::NoCheck`)
- **TODO**: Implement proper host key verification for production use
- Use encrypted private keys when possible
- Consider using SSH agent authentication (coming soon)
- The connection is tunneled through SSH with full encryption

## Implementation Details

### russh vs async-ssh2-tokio

This implementation uses `russh`, a pure Rust SSH library:
- ✅ No C dependencies (no libssh2)
- ✅ Better async/await integration
- ✅ More idiomatic Rust API
- ✅ Easier to cross-compile
- ✅ Active maintenance
    --ssh "user@remote.example.com:22/tmp/ractor_wormhole_sample_app" \
    --ssh-key "/home/user/.ssh/id_rsa" \
    --ssh-key-passphrase "key_passphrase"
```

## How It Works

The SSH transport works by:

1. Establishing an SSH connection to the remote host
2. For Unix sockets: Executing `socat STDIO UNIX-CONNECT:/path/to/socket` on the remote host to bridge the SSH channel to the Unix socket
3. For TCP ports: Using SSH's direct-tcpip channel forwarding
4. Creating Conduit sink/source pairs that read/write through the SSH channel
5. Registering the connection with the Nexus as a Portal

## Requirements

### For Unix Socket Tunneling

The remote machine must have `socat` installed:

```bash
# Debian/Ubuntu
sudo apt-get install socat

# RHEL/CentOS
sudo yum install socat

# macOS
brew install socat
```

### For TCP Port Tunneling

No additional requirements - uses SSH's built-in port forwarding capabilities.

## Security Considerations with known_hosts
- [ ] SSH agent authentication support  
- [ ] Connection pooling and reuse (partially done - you can reuse Handle)
- [ ] Better error handling and auto-reconnection
- [ ] Compression support
- [ ] Keep-alive configuration
- [ ] WebSocket-over-TCP-over-SSH adapter example is tunneled through SSH, adding encryption overhead

## Protocol

The SSH transport uses the same binary framing protocol as the Unix socket transport:

```
[type:1byte][length:4bytes][data]
```

Where type can be:
- `0`: Text message
- `1`: Binary message
- `2`: Close message

## Future Improvements

- [ ] Proper SSH host key verification
- [ ] SSH agent authentication support
- [ ] Direct Unix socket forwarding (without socat dependency)
- [ ] Connection pooling and reuse
- [ ] Better error handling and reconnection logic
- [ ] Compression support
- [ ] Keep-alive configuration
