const fs = require('node:fs');
const path = require('node:path');
const SAMPLE_RATE = 16000, MAX_SECONDS = 60, MIN_SECONDS = 0.4;
function wav(pcm) {
  const h = Buffer.alloc(44);
  h.write('RIFF', 0); h.writeUInt32LE(36 + pcm.length, 4); h.write('WAVE', 8); h.write('fmt ', 12);
  h.writeUInt32LE(16, 16); h.writeUInt16LE(1, 20); h.writeUInt16LE(1, 22); h.writeUInt32LE(SAMPLE_RATE, 24);
  h.writeUInt32LE(SAMPLE_RATE * 2, 28); h.writeUInt16LE(2, 32); h.writeUInt16LE(16, 34); h.write('data', 36); h.writeUInt32LE(pcm.length, 40);
  return Buffer.concat([h, pcm]);
}
function level(pcm) {
  let peak = 0, sum = 0; const n = pcm.length >> 1;
  for (let i = 0; i < n; i++) { const v = pcm.readInt16LE(i * 2); peak = Math.max(peak, Math.abs(v)); sum += v * v; }
  return { peak, rms: Math.round(Math.sqrt(sum / Math.max(1, n))) };
}
// One recording at a time: the board streams 16 kHz mono 16-bit PCM between start and stop.
class Voice {
  constructor(dir, recognize) { this.dir = dir; this.recognize = recognize; this.chunks = null; this.bytes = 0; }
  start() { this.chunks = []; this.bytes = 0; }
  audio(buf) { if (!this.chunks || this.bytes + buf.length > SAMPLE_RATE * 2 * MAX_SECONDS) return; this.chunks.push(Buffer.from(buf)); this.bytes += buf.length; }
  async stop(reply) {
    if (!this.chunks) return;
    const pcm = Buffer.concat(this.chunks); this.chunks = null;
    const seconds = pcm.length / 2 / SAMPLE_RATE, stats = level(pcm);
    fs.mkdirSync(this.dir, { recursive: true }); fs.writeFileSync(path.join(this.dir, 'last.wav'), wav(pcm));
    console.log(JSON.stringify({ event: 'voice', seconds: Number(seconds.toFixed(2)), ...stats, time: new Date().toISOString() }));
    if (seconds < MIN_SECONDS) return reply({ type: 'voice', state: 'error', text: '说话时间太短' });
    reply({ type: 'voice', state: 'recognizing' });
    try {
      const text = (await this.recognize(pcm, { seconds, ...stats })).trim();
      reply(text ? { type: 'voice', state: 'done', text } : { type: 'voice', state: 'error', text: '没听清，再说一次' });
    } catch (e) { reply({ type: 'voice', state: 'error', text: '识别失败' }); console.log(JSON.stringify({ event: 'voice-error', message: e.message })); }
  }
}
module.exports = { Voice, wav, level, SAMPLE_RATE };
