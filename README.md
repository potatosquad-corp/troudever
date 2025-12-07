# Troudever

Troudever is a lightweight, high-performance proxy designed to bridge the gap between modern Web Clients and legacy or raw TCP services.

The name implies a "Wormhole" (Trou de ver in French), shortcutting the distance between web protocols and raw sockets. It stands for:

*Tunnel Relay Offering Unrestricted Duplex Exchange Via Encapsulated Routes*

## 🚀 What it does

Browsers speak WebSockets; many backend services speak Raw TCP Sockets. Troudever sits in the middle, handling the handshake and framing so your web apps can talk directly to database ports, game servers, or IoT devices without modifying the backend code.

```mermaid
sequenceDiagram
    participant ws-server as ws-server (Localhost)
    participant Rust as Troudever (Proxy)
    participant Server as Relay server (Cloud)
    participant Web as Web App (Client)

    Note over ws-server, Rust:  Websocket (Insecure)
    Note over Rust, Server: TCP Socket 
    Note over Server,Web:  Websocket (Secure)

    Rust->>ws-server: Connexion
    Rust->>ws-server: Authentification
    
    Rust->>Server: Create Room 
    Web->>Server: Join Room

    par Receiving events
        ws-server->>Rust: Event
        Rust->>Server: Forward Event
        Server->>Web: Broadcast Event
    and Sending a command
        Web->>Server: Command
        Server->>Rust: Forward Command
        Rust->>ws-server: Execute Command
    end
```

## ⚡ Key Features
- Protocol Translation: Transparently decapsulates WebSocket frames into raw TCP streams (and vice-versa).
- Full Duplex: True bidirectional communication suitable for real-time applications.
- Zero-Copy Relay: Optimized for low latency and high throughput.
- Agnostic: Works with any TCP target (Redis, SSH, Custom C++ Servers, etc.).

# Quick Start

```Bash
./troudever 
```
