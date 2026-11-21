'use strict';
const fs = require('node:fs');
const net = require('node:net');
const http = require('node:http');
const https = require('node:https');
const [cert, key, mode] = process.argv.slice(2);
const options = {cert: fs.readFileSync(cert), key: fs.readFileSync(key)};
let nextConnection = 0;
const connections = new WeakMap();
const origin = https.createServer(options, (req, res) => {
    let bytes = 0;
    req.on('data', data => bytes += data.length);
    req.on('end', () => {
        if (!connections.has(req.socket)) connections.set(req.socket, ++nextConnection);
        const body = `origin ${req.url} auth=${!!req.headers['proxy-authorization']} bytes=${bytes} conn=${connections.get(req.socket)}`;
        res.writeHead(200, {'Content-Length': Buffer.byteLength(body)});
        res.end(body);
    });
});
origin.on('tlsClientError', () => {});
origin.listen(0, '127.0.0.1', () => {
    const proxy = mode === 'http-proxy' ? http.createServer() : https.createServer(options);
    proxy.on('tlsClientError', () => {});
    proxy.on('request', (req, res) => {
        const authorized = req.headers['proxy-authorization'] === 'Basic cHJveHk6c2VjcmV0';
        if (!authorized) { res.writeHead(407); res.end(); return; }
        const body = `proxy ${req.url} auth=${authorized}`;
        res.writeHead(200, {'Content-Length': Buffer.byteLength(body)});
        res.end(body);
    });
    proxy.on('connect', (req, socket, head) => {
        socket.on('error', () => {});
        if (req.headers['proxy-authorization'] !== 'Basic cHJveHk6c2VjcmV0') {
            socket.end('HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n');
            return;
        }
        if (mode === 'stall') return;
        if (mode === 'refused') {
            socket.end('HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n');
            return;
        }
        const upstream = net.connect(origin.address().port, '127.0.0.1', () => {
            socket.write('HTTP/1.1 200 Connection Established\r\n\r\n');
            if (head.length) upstream.write(head);
            socket.pipe(upstream);
            upstream.pipe(socket);
        });
        upstream.on('error', () => socket.destroy());
        socket.on('close', () => upstream.destroy());
        upstream.on('close', () => socket.destroy());
    });
    proxy.listen(0, '127.0.0.1', () => console.log(`${proxy.address().port} ${origin.address().port}`));
});
