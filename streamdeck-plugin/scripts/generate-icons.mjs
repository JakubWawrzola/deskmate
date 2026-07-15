// Generator placeholderowych ikon PNG dla pluginu Stream Deck.
// Czysty Node (zlib) — zadnych zaleznosci, zadnego pobierania z sieci.
// Uruchomienie: npm run icons (albo: node scripts/generate-icons.mjs)

import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const IMGS = join(__dirname, "..", "com.homeos.homeassistant.sdPlugin", "imgs");

// ---------- kodowanie PNG ----------

const CRC_TABLE = (() => {
	const table = new Int32Array(256);
	for (let n = 0; n < 256; n++) {
		let c = n;
		for (let k = 0; k < 8; k++) {
			c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
		}
		table[n] = c;
	}
	return table;
})();

function crc32(buf) {
	let c = 0xffffffff;
	for (let i = 0; i < buf.length; i++) {
		c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
	}
	return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
	const out = Buffer.alloc(12 + data.length);
	out.writeUInt32BE(data.length, 0);
	out.write(type, 4, "ascii");
	data.copy(out, 8);
	out.writeUInt32BE(crc32(out.subarray(4, 8 + data.length)), 8 + data.length);
	return out;
}

function encodePng(size, rgba) {
	const ihdr = Buffer.alloc(13);
	ihdr.writeUInt32BE(size, 0);
	ihdr.writeUInt32BE(size, 4);
	ihdr[8] = 8; // bit depth
	ihdr[9] = 6; // RGBA
	const raw = Buffer.alloc((size * 4 + 1) * size);
	for (let y = 0; y < size; y++) {
		const rowStart = y * (size * 4 + 1);
		raw[rowStart] = 0; // filtr: none
		rgba.copy(raw, rowStart + 1, y * size * 4, (y + 1) * size * 4);
	}
	return Buffer.concat([
		Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
		chunk("IHDR", ihdr),
		chunk("IDAT", deflateSync(raw, { level: 9 })),
		chunk("IEND", Buffer.alloc(0))
	]);
}

// ---------- rysowanie (supersampling 3x3, wspolrzedne logiczne 0..72) ----------

function render(size, paint) {
	const ss = 3;
	const rgba = Buffer.alloc(size * size * 4);
	const scale = 72 / size;
	for (let py = 0; py < size; py++) {
		for (let px = 0; px < size; px++) {
			let r = 0, g = 0, b = 0, a = 0;
			for (let sy = 0; sy < ss; sy++) {
				for (let sx = 0; sx < ss; sx++) {
					const lx = (px + (sx + 0.5) / ss) * scale;
					const ly = (py + (sy + 0.5) / ss) * scale;
					const c = paint(lx, ly);
					if (c) {
						r += c[0];
						g += c[1];
						b += c[2];
						a += c[3];
					}
				}
			}
			const n = ss * ss;
			const i = (py * size + px) * 4;
			// kolory usredniane z waga alfa (tlo przezroczyste)
			if (a > 0) {
				rgba[i] = Math.round(r / n);
				rgba[i + 1] = Math.round(g / n);
				rgba[i + 2] = Math.round(b / n);
				rgba[i + 3] = Math.round(a / n);
			}
		}
	}
	return encodePng(size, rgba);
}

// predykaty geometryczne
const inRoundRect = (x, y, rx, ry, w, h, r) => {
	const qx = Math.abs(x - (rx + w / 2)) - (w / 2 - r);
	const qy = Math.abs(y - (ry + h / 2)) - (h / 2 - r);
	return Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) - r <= 0;
};

const inCircle = (x, y, cx, cy, r) => Math.hypot(x - cx, y - cy) <= r;

const distSeg = (x, y, x1, y1, x2, y2) => {
	const dx = x2 - x1;
	const dy = y2 - y1;
	const len2 = dx * dx + dy * dy;
	const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((x - x1) * dx + (y - y1) * dy) / len2));
	return Math.hypot(x - (x1 + t * dx), y - (y1 + t * dy));
};

const inPolygon = (x, y, pts) => {
	let inside = false;
	for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) {
		const [xi, yi] = pts[i];
		const [xj, yj] = pts[j];
		if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) {
			inside = !inside;
		}
	}
	return inside;
};

// symbole (przestrzen logiczna 72x72)
const powerSymbol = (x, y) => {
	// pierscien z przerwa u gory + pionowa kreska
	const d = Math.hypot(x - 36, y - 40);
	if (Math.abs(d - 13) <= 2.75) {
		const angle = (Math.atan2(x - 36, -(y - 40)) * 180) / Math.PI;
		if (Math.abs(angle) > 50) {
			return true;
		}
	}
	return distSeg(x, y, 36, 17, 36, 37) <= 2.75;
};

const chevronSymbol = (x, y) =>
	distSeg(x, y, 23, 25, 34, 36) <= 2.6 ||
	distSeg(x, y, 34, 36, 23, 47) <= 2.6 ||
	distSeg(x, y, 40, 46, 51, 46) <= 2.6;

const sunSymbol = (x, y) => {
	if (inCircle(x, y, 36, 36, 9)) {
		return true;
	}
	for (let i = 0; i < 8; i++) {
		const a = (i * Math.PI) / 4;
		const x1 = 36 + 13 * Math.cos(a);
		const y1 = 36 + 13 * Math.sin(a);
		const x2 = 36 + 18 * Math.cos(a);
		const y2 = 36 + 18 * Math.sin(a);
		if (distSeg(x, y, x1, y1, x2, y2) <= 2.2) {
			return true;
		}
	}
	return false;
};

const houseSymbol = (x, y) => {
	if (inPolygon(x, y, [[36, 12], [11, 35], [61, 35]])) {
		return true;
	}
	if (x >= 19 && x <= 53 && y >= 35 && y <= 58) {
		// wyciecie drzwi
		return !(x >= 31 && x <= 41 && y >= 44 && y <= 58);
	}
	return false;
};

// palety (monochrom jak UI Deskmate)
const WHITE = [255, 255, 255, 255];
const NEAR_BLACK = [17, 17, 19, 255];
const DIM_GRAY = [122, 122, 128, 255];
const BG_DARK = [25, 25, 27, 255];
const BG_LIGHT = [245, 245, 245, 255];

// klawisz: ciemne zaokraglone tlo + symbol
const key = (symbol, bg, fg) => (x, y) => {
	if (!inRoundRect(x, y, 3, 3, 66, 66, 14)) {
		return null;
	}
	return symbol(x, y) ? fg : bg;
};

// ikona do listy akcji: sam symbol, bez tla
const bare = (symbol, fg) => (x, y) => (symbol(x, y) ? fg : null);

// ---------- pliki ----------

const files = [
	// ikona pluginu (marketplace) + ikona kategorii
	["plugin/marketplace.png", 72, key(houseSymbol, BG_DARK, WHITE)],
	["plugin/marketplace@2x.png", 144, key(houseSymbol, BG_DARK, WHITE)],
	["plugin/category-icon.png", 28, bare(houseSymbol, WHITE)],
	["plugin/category-icon@2x.png", 56, bare(houseSymbol, WHITE)],

	// Toggle Entity: OFF = przygaszony na ciemnym, ON = czarny symbol na jasnym
	["actions/toggle/icon.png", 20, bare(powerSymbol, WHITE)],
	["actions/toggle/icon@2x.png", 40, bare(powerSymbol, WHITE)],
	["actions/toggle/off.png", 72, key(powerSymbol, BG_DARK, DIM_GRAY)],
	["actions/toggle/off@2x.png", 144, key(powerSymbol, BG_DARK, DIM_GRAY)],
	["actions/toggle/on.png", 72, key(powerSymbol, BG_LIGHT, NEAR_BLACK)],
	["actions/toggle/on@2x.png", 144, key(powerSymbol, BG_LIGHT, NEAR_BLACK)],

	// Call Service: chevron
	["actions/service/icon.png", 20, bare(chevronSymbol, WHITE)],
	["actions/service/icon@2x.png", 40, bare(chevronSymbol, WHITE)],
	["actions/service/key.png", 72, key(chevronSymbol, BG_DARK, WHITE)],
	["actions/service/key@2x.png", 144, key(chevronSymbol, BG_DARK, WHITE)],

	// Activate Scene: slonce
	["actions/scene/icon.png", 20, bare(sunSymbol, WHITE)],
	["actions/scene/icon@2x.png", 40, bare(sunSymbol, WHITE)],
	["actions/scene/key.png", 72, key(sunSymbol, BG_DARK, WHITE)],
	["actions/scene/key@2x.png", 144, key(sunSymbol, BG_DARK, WHITE)]
];

for (const [rel, size, paint] of files) {
	const out = join(IMGS, rel);
	mkdirSync(dirname(out), { recursive: true });
	writeFileSync(out, render(size, paint));
	console.log(`written ${rel} (${size}x${size})`);
}
console.log("done");
