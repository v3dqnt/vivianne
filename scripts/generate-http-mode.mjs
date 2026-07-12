import fs from 'fs';
import path from 'path';

const rpcPath = path.join('packages', 'coding-agent', 'src', 'modes', 'rpc', 'rpc-mode.ts');
const httpPath = path.join('packages', 'coding-agent', 'src', 'modes', 'http', 'http-mode.ts');

let content = fs.readFileSync(rpcPath, 'utf8');

content = content.replace(/runRpcMode/g, 'runHttpMode');
content = content.replace(/export async function runHttpMode\(runtimeHost: AgentSessionRuntime\): Promise<never> \{/, `import * as http from 'node:http';
export async function runHttpMode(runtimeHost: AgentSessionRuntime, port: number): Promise<never> {
	const clients = new Set<http.ServerResponse>();
	const output = (obj: any) => {
		const data = JSON.stringify(obj);
		for (const client of clients) {
			client.write(\`data: \${data}\\n\\n\`);
		}
	};`);

content = content.replace(/import \{[\s\S]*?waitForRawStdoutBackpressure,\s*writeRawStdout,\s*\} from "\.\.\/\.\.\/core\/output-guard\.ts";/, '');
content = content.replace(/import \{ attachJsonlLineReader, serializeJsonLine \} from "\.\/jsonl\.ts";/, '');

const splitPoint = content.indexOf('const handleInputLine = async');
if (splitPoint !== -1) {
    content = content.substring(0, splitPoint);
}

content += `
	const server = http.createServer((req, res) => {
		res.setHeader('Access-Control-Allow-Origin', '*');
		res.setHeader('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
		res.setHeader('Access-Control-Allow-Headers', 'Content-Type');

		if (req.method === 'OPTIONS') {
			res.writeHead(200);
			res.end();
			return;
		}

		if (req.url === '/api/stream' && req.method === 'GET') {
			res.writeHead(200, {
				'Content-Type': 'text/event-stream',
				'Cache-Control': 'no-cache',
				'Connection': 'keep-alive'
			});
			clients.add(res);
			req.on('close', () => {
				clients.delete(res);
			});
			return;
		}

		if (req.url === '/api/command' && req.method === 'POST') {
			let body = '';
			req.on('data', chunk => { body += chunk.toString(); });
			req.on('end', async () => {
				try {
					const parsed = JSON.parse(body);
					
					// Handle extension UI responses directly
					if (parsed && typeof parsed === 'object' && parsed.type === 'extension_ui_response') {
						const response = parsed as RpcExtensionUIResponse;
						const pending = pendingExtensionRequests.get(response.id);
						if (pending) {
							pendingExtensionRequests.delete(response.id);
							pending.resolve(response);
						}
						res.writeHead(200, { 'Content-Type': 'application/json' });
						res.end(JSON.stringify({ success: true }));
						return;
					}

					const command = parsed as RpcCommand;
					const response = await handleCommand(command);
					res.writeHead(200, { 'Content-Type': 'application/json' });
					res.end(JSON.stringify(response || { success: true }));
					await checkShutdownRequested();
				} catch (e) {
					res.writeHead(500, { 'Content-Type': 'application/json' });
					res.end(JSON.stringify({ error: e instanceof Error ? e.message : String(e) }));
				}
			});
			return;
		}

		res.writeHead(404);
		res.end('Not found');
	});

	server.listen(port, () => {
		console.log(\`Agent HTTP server listening on port \${port}\`);
	});

	// Keep process alive forever
	return new Promise(() => {});
}
`;

fs.writeFileSync(httpPath, content);
console.log('Created http-mode.ts');
