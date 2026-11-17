'use strict';
const fs = require('fs');
const http2 = require('http2');
const https = require('https');

const [cert, key, mode] = process.argv.slice(2);
let sessions = 0;
let inflight = 0;
let peak = 0;
const parked = [];

function handle(req, res) {
  const chunks = [];
  req.on('data', (chunk) => chunks.push(chunk));
  req.on('end', () => {
    const size = Buffer.concat(chunks).length;
    const url = new URL(req.url, 'https://localhost');
    const reply = (text) => {
      res.setHeader('content-type', 'text/plain');
      res.end(text);
    };
    if (url.pathname === '/slow') {
      inflight += 1;
      peak = Math.max(peak, inflight);
      parked.push(() => {
        inflight -= 1;
        reply(`${req.httpVersion} ${url.searchParams.get('n')}`);
      });
      const release = () => parked.splice(0).forEach((answer) => answer());
      if (inflight === 5) {
        release();
      } else {
        setTimeout(release, 5000);
      }
    } else if (url.pathname === '/peak') {
      reply(String(peak));
    } else if (url.pathname === '/sessions') {
      reply(String(sessions));
    } else if (url.pathname === '/big') {
      reply('x'.repeat(300000));
    } else if (url.pathname === '/echo') {
      reply(`${req.httpVersion} ${req.method} ${size}`);
    } else if (url.pathname === '/header') {
      reply(`${req.headers['x-trace']}`);
    } else {
      res.statusCode = 404;
      reply('missing');
    }
  });
}

const options = { cert: fs.readFileSync(cert), key: fs.readFileSync(key) };
let server;
if (mode === 'http1') {
  server = https.createServer(options, handle);
} else {
  server = http2.createSecureServer({ ...options, allowHTTP1: true }, handle);
  server.on('session', () => { sessions += 1; });
}
server.listen(0, '127.0.0.1', () => {
  process.stdout.write(`${server.address().port}\n`);
});
process.stdin.on('end', () => process.exit(0));
process.stdin.resume();
