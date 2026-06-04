const { Resvg } = require('@resvg/resvg-js');
const fs = require('fs');
const path = require('path');

const svgPath = path.join(__dirname, '../src-tauri/icons/source.svg');
const outPath = path.join(__dirname, '../src-tauri/icons/icon.png');

const svg = fs.readFileSync(svgPath, 'utf8');
const resvg = new Resvg(svg, { fitTo: { mode: 'width', value: 1024 } });
const rendered = resvg.render();
fs.writeFileSync(outPath, rendered.asPng());
console.log('Generated icon.png (1024x1024)');
