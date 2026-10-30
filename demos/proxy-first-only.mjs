// Tiny CONNECT proxy for the TUI recording: tunnels the FIRST connection only (the
// country list) and refuses the rest, so the exit-IP lookup fails and the stats panel
// shows "unknown" instead of the recorder's real IP. Port from $1.
import net from "node:net";
let first = true;
net.createServer((c) => {
  c.once("data", (buf) => {
    const m = /^CONNECT ([^:\s]+):(\d+)/.exec(buf.toString());
    if (!m || !first) { c.end("HTTP/1.1 403 Forbidden\r\n\r\n"); return; }
    first = false;
    const u = net.connect(+m[2], m[1], () => {
      c.write("HTTP/1.1 200 OK\r\n\r\n");
      c.pipe(u); u.pipe(c);
    });
    u.on("error", () => c.destroy()); c.on("error", () => u.destroy());
  });
}).listen(+process.argv[2], "127.0.0.1");
