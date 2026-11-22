# Driving Plotter Studio from an AI assistant or a script (MCP)

Plotter Studio can serve its features to any [Model Context Protocol](https://modelcontextprotocol.io)
client: an AI assistant, a script, an inspector tool. The client sees a dozen tools (for example
`project`, `import`, `object`, `operation`, `compile`, `export`, `machine` and `query`) that do what
the window does, and the window follows along.

The server uses MCP's **Streamable HTTP** transport. It is off until you turn it on.

## Turning it on

Open **Settings**, find **Automation**, and tick **Enable the MCP server**. The settings then show:

- **Port** (default 8765).
- **Allow other computers on the network to connect** (off by default).
- **Connect your client to:** the address or addresses to use. With the network option off that
  is `http://127.0.0.1:8765/mcp`; with it on, one address per network the computer is on is listed
  first, such as `http://192.168.1.20:8765/mcp`.
- **Access token:** a secret every request must carry. **Show token**, **Copy token** and **Make a
  new token** are next to it. A new token stops the old one working at once.

The setting is remembered. The server runs while the app does, and stops when the app closes or
when you untick the box.

## Connecting a client

Send the token as a bearer token. For example, with Claude Code:

```sh
claude mcp add --transport http plotter-studio http://127.0.0.1:8765/mcp \
  --header "Authorization: Bearer <your token>"
```

Other clients take the same two pieces, a URL and an `Authorization: Bearer <token>` header, in
their own configuration, for example:

```json
{
  "mcpServers": {
    "plotter-studio": {
      "type": "http",
      "url": "http://127.0.0.1:8765/mcp",
      "headers": { "Authorization": "Bearer <your token>" }
    }
  }
}
```

To check the connection by hand:

```sh
curl -s http://127.0.0.1:8765/mcp \
  -H "Authorization: Bearer <your token>" -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

## What the server does

`POST /mcp` takes one JSON-RPC message and answers with the JSON reply, or with `202 Accepted` and
no body for a notification. There are no sessions and the server never sends messages on its own,
so `GET` and `DELETE` answer `405`. Batches are not supported (the 2025-06-18 revision of the
protocol removed them). The server accepts protocol revisions 2024-11-05, 2025-03-26 and
2025-06-18.

Tool calls run in the app, between frames, on the same project you see. A call that takes longer
than 30 seconds to be picked up answers `504`.

## Safety

The tools can edit your project and, with your permission, drive a real machine, so the server is
closed by default:

- **Off until you enable it**, and **this computer only** until you allow the network.
- **Every request needs the token.** Without it, the answer is `401` and nothing reaches the app.
  The token is 256 random bits and is kept in `settings.toml` in your user configuration folder.
  Treat it like a password.
- **Web pages cannot use it.** A request is refused when its `Host` is not a name for this
  computer (while the network is off) or when it carries an `Origin` from another website, so a
  page you visit cannot reach the server through your browser, even by DNS rebinding.
- **No encryption.** The connection is plain HTTP. Do not allow the network on a network you do
  not trust: anyone who can watch it can read the token. Put the server behind a TLS proxy or an
  SSH tunnel if you need to reach it across the internet.
- **Moving the machine is a separate switch.** Without it the tools prepare, compile, preview and
  run on the simulator, but never move real hardware. Tick **Let the API move the machine until the
  app is closed** in the same settings section (or start the app with `--arm-api`). It is not
  remembered, and no tool can turn it on. Be at the machine, near the e-stop, when you do.

## For developers

The transport is [`crates/ps-app/src/mcp/http.rs`](../crates/ps-app/src/mcp/http.rs); the message
handling, which has no I/O, is in [`mcp.rs`](../crates/ps-app/src/mcp.rs). Tests start the server
on a free port and talk to it over a real socket.
