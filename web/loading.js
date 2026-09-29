// Browser runtime transfer only; game asset progress is owned by Rust/Bevy.
export async function fetchWasm(url, expectedBytes, onProgress) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`Game download failed (HTTP ${response.status}). Reload to retry.`);
  const report = received => onProgress(Math.min(19, Math.floor(20 * received / expectedBytes)));
  onProgress(0);
  if (!response.body) {
    const bytes = await response.arrayBuffer();
    report(bytes.byteLength);
    return new Response(bytes, { headers: { 'Content-Type': 'application/wasm' } });
  }
  const reader = response.body.getReader();
  const chunks = [];
  let received = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      chunks.push(value);
      received += value.byteLength;
      report(received);
    }
  } finally {
    reader.releaseLock();
  }
  // The generated build size is the decoded WASM size, also correct with HTTP compression.
  return new Response(new Blob(chunks, { type: 'application/wasm' }));
}
