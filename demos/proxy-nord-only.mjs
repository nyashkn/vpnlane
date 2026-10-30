// Tiny proxy for the TUI recording. The HTTPS API calls (country and server lists) arrive as CONNECT
// and are tunnelled to api.nordvpn.com only; everything else, including the plain-HTTP exit-IP lookup,
// is refused, so the stats panel shows "unknown" instead of the recorder's real IP. Port from $1.
import net from "node:net";
net.createServer((c) => {
  c.once("data", (buf) => {
    const m = /^CONNECT ([^:\s]+):(\d+)/.exec(buf.toString());
    if (!m || m[1] !== "api.nordvpn.com") { c.end("HTTP/1.1 403 Forbidden\r\n\r\n"); return; }
    const u = net.connect(+m[2], m[1], () => {
      c.write("HTTP/1.1 200 OK\r\n\r\n");
      c.pipe(u); u.pipe(c);
    });
    u.on("error", () => c.destroy()); c.on("error", () => u.destroy());
  });
}).listen(+process.argv[2], "127.0.0.1");
