const sharp = require('sharp');
const path = require('path');

const input  = path.join(__dirname, '../src-tauri/icons/maple_source.png');
const output = path.join(__dirname, '../src-tauri/icons/icon.png');

async function main() {
  const SIZE = 1024;
  const { data, info } = await sharp(input)
    .resize(SIZE, SIZE, { fit: 'contain', background: { r: 255, g: 255, b: 255, alpha: 255 } })
    .ensureAlpha()
    .raw()
    .toBuffer({ resolveWithObject: true });

  const buf = Buffer.from(data);
  const W = info.width, H = info.height, C = 4;
  function idx(x, y) { return (y * W + x) * C; }

  // Flood-fill from corners. Low tolerance so it eats ONLY the outer white,
  // stopping at the card's gray rounded-rect border (which stays intact).
  const TOLERANCE = 90;
  const visited = new Uint8Array(W * H);
  const queue = [];
  for (const [sx, sy] of [[0,0],[W-1,0],[0,H-1],[W-1,H-1]]) {
    if (!visited[sy*W+sx]) {
      const si = idx(sx, sy);
      queue.push([sx, sy, buf[si], buf[si+1], buf[si+2]]);
      visited[sy*W+sx] = 1;
    }
  }
  while (queue.length) {
    const [x, y, tr, tg, tb] = queue.pop();
    const i = idx(x, y);
    const diff = Math.abs(buf[i]-tr) + Math.abs(buf[i+1]-tg) + Math.abs(buf[i+2]-tb);
    if (diff > TOLERANCE) continue;
    buf[i+3] = 0;
    for (const [nx, ny] of [[x-1,y],[x+1,y],[x,y-1],[x,y+1]]) {
      if (nx < 0 || ny < 0 || nx >= W || ny >= H) continue;
      if (visited[ny*W+nx]) continue;
      visited[ny*W+nx] = 1;
      queue.push([nx, ny, tr, tg, tb]);
    }
  }

  // Output transparent PNG (keep the white card, only outer ring removed)
  await sharp(buf, { raw: { width: W, height: H, channels: C } })
    .png()
    .toFile(output);

  console.log('Done:', output);
}

main().catch(e => { console.error(e); process.exit(1); });
