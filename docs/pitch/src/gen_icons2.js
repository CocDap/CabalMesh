const React = require('react');
const ReactDOMServer = require('react-dom/server');
const sharp = require('sharp');
const fs = require('fs');
const path = require('path');

const fa = require('react-icons/fa');
const fa6 = require('react-icons/fa6');

const OUT = path.join(__dirname, 'icons2');
fs.mkdirSync(OUT, { recursive: true });

const icons = {
  network: fa.FaNetworkWired,
  brain: fa.FaBrain,
  link: fa.FaLink,
  eye_slash: fa.FaEyeSlash,
  fingerprint: fa6.FaFingerprint,
  users: fa.FaUsers,
  store: fa.FaStore,
  shield: fa.FaShieldAlt,
  route: fa.FaRoute,
  robot: fa.FaRobot,
  lock: fa.FaLock,
  key: fa.FaKey,
  calendar: fa.FaRegCalendarAlt,
  flag: fa.FaFlagCheckered,
  github: fa.FaGithub,
  play: fa.FaPlay,
  bolt: fa.FaBolt,
  chart_line: fa.FaChartLine,
  arrow_right: fa.FaArrowRight,
  cube: fa.FaCube,
};

const colors = { ink: '19140F', white: 'FFFFFF', accent: 'E8541A', muted: '9A8F80' };

async function run() {
  for (const [name, Comp] of Object.entries(icons)) {
    for (const [cname, hex] of Object.entries(colors)) {
      const svg = ReactDOMServer.renderToStaticMarkup(React.createElement(Comp, { size: 256, color: `#${hex}` }));
      const svgBuf = Buffer.from(svg);
      await sharp(svgBuf, { density: 384 }).resize(256, 256).png().toFile(path.join(OUT, `${name}_${cname}.png`));
    }
  }
  console.log('done', Object.keys(icons).length * Object.keys(colors).length, 'files');
}
run();
