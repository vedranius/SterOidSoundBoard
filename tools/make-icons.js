// App icons for the browser edition (PWA): blue tile with three sound waves,
// the motif of the header logo. Renders with the preinstalled Chromium:
//   node tools/make-icons.js
const path = require("path"), fs = require("fs");
const { chromium } = require(require("child_process").execSync("npm root -g").toString().trim() + "/playwright");
const out = path.join(__dirname, "..", "web", "local", "icons");
fs.mkdirSync(out, { recursive: true });
(async () => {
  const b = await chromium.launch(), p = await b.newPage();
  const draw = (size, maskable) => p.evaluate(([size, maskable]) => {
    const c = document.createElement("canvas"); c.width = c.height = size;
    const g = c.getContext("2d"), S = size;
    g.fillStyle = "#1c5da8";
    if (maskable) g.fillRect(0, 0, S, S);
    else { g.beginPath(); g.roundRect(0, 0, S, S, S * 0.22); g.fill(); }
    const pad = S * (maskable ? 0.22 : 0.16), w = S - 2 * pad;
    g.strokeStyle = "#fff"; g.lineWidth = S * 0.055; g.lineCap = "round"; g.lineJoin = "round";
    [0.3, 0.5, 0.7].forEach((y, k) => {
      const amp = w * (k === 1 ? 0.11 : 0.07);
      g.beginPath();
      for (let t = 0; t <= 200; t++) { const x = pad + (w * t) / 200, yy = pad + w * y + amp * Math.sin((t / 200) * 2 * Math.PI * 2); t ? g.lineTo(x, yy) : g.moveTo(x, yy); }
      g.stroke();
    });
    return c.toDataURL("image/png").split(",")[1];
  }, [size, maskable]);
  const save = async (name, size, m) => fs.writeFileSync(path.join(out, name), Buffer.from(await draw(size, m), "base64"));
  await save("icon-192.png", 192, false);
  await save("icon-512.png", 512, false);
  await save("maskable-512.png", 512, true);
  await save("apple-touch-icon.png", 180, false);
  await save("favicon-32.png", 32, false);
  await b.close();
  console.log("icons written to", out);
})();
