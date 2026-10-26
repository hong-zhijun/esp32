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
  constructor(dir, recognize, typeText = async () => {}, { live = false, log = () => {} } = {}) { this.log = log; this.live = live; this.dir = dir; this.recognize = recognize; this.typeText = typeText; this.chunks = null; this.bytes = 0; this.generation = 0; this.previewBusy = false; this.lastPreviewBytes = 0; this.previewReply = null; }
  start(reply = null) { this.log({event:'recording-start'}); this.lastLevelLog=0; this.startedAt = Date.now(); this.lastAudioAt = 0; this.maxAudioGapMs = 0; this.generation++; this.chunks = []; this.bytes = 0; this.lastPreviewBytes = 0; this.previewReply = reply; this.segment = { chunks: [], bytes: 0, silence: 0, speech: 0, text: '', error: null, queue: Promise.resolve(), generation: this.generation, reply }; }
  commitSegment(session) {
    if (!session.bytes) return;
    const pcm = Buffer.concat(session.chunks), speech = session.speech;
    session.chunks = []; session.bytes = 0; session.silence = 0; session.speech = 0;
    if (speech < SAMPLE_RATE * 2 * 0.2) return;
    const queuedAt = Date.now();
    session.queue = session.queue.then(async () => {
      const startedAt = Date.now();
      if (session.error || session.generation !== this.generation) return;
      this.log({event:'recognition-start',audioSeconds:pcm.length/32000,queueMs:startedAt-queuedAt});
      let text;
      try { text = (await this.recognize(pcm)).trim(); }
      catch (e) { session.error = '语音识别失败：' + e.message; this.log({event:'voice-error',stage:'recognize',message:e.message}); return; }
      if (!text || session.generation !== this.generation) return;
      const recognizedAt = Date.now();
      this.log({event:'recognized',text,recognizeMs:recognizedAt-startedAt});
      // Add an English word separator, without introducing spaces between Chinese phrases.
      const separator = /[a-zA-Z0-9]$/.test(session.text) && /^[a-zA-Z0-9]/.test(text) ? ' ' : '';
      try { await this.typeText(separator + text); }
      catch (e) { session.error = '自动输入失败：' + text; this.log({event:'voice-error',stage:'type',message:e.message}); return; }
      this.log({event:'voice-segment-timing',audioSeconds:pcm.length/32000,queueMs:startedAt-queuedAt,recognizeMs:recognizedAt-startedAt,pasteMs:Date.now()-recognizedAt});
      session.text += separator + text;
      if (session.generation === this.generation && this.chunks && session.reply) session.reply({ type: 'voice', state: 'partial', text: session.text.slice(-220) });
    });
  }
  liveAudio(buf) {
    const session = this.segment;
    session.chunks.push(buf); session.bytes += buf.length;
    if (level(buf).rms >= 120) { session.speech += buf.length; session.silence = 0; }
    else session.silence += buf.length;
    if (session.bytes >= SAMPLE_RATE * 2 * 2 || (session.speech && session.silence >= SAMPLE_RATE * 2 * 0.6)) this.commitSegment(session);
  }
  async preview() {
    if (!this.previewReply || this.previewBusy || !this.chunks || this.bytes - this.lastPreviewBytes < SAMPLE_RATE * 2 * 1.5) return;
    this.previewBusy = true; this.lastPreviewBytes = this.bytes;
    const generation = this.generation, reply = this.previewReply;
    // Bound preview cost; final recognition still uses the whole recording.
    const pcm = Buffer.concat(this.chunks).subarray(-SAMPLE_RATE * 2 * 12);
    try {
      const text = (await this.recognize(pcm)).trim();
      if (generation === this.generation && this.chunks && text) reply({ type: 'voice', state: 'partial', text });
    } catch (e) {
      this.log({ event: 'voice-preview-error', message: e.message });
    } finally { this.previewBusy = false; }
  }
  audio(buf) { if (!this.chunks || this.bytes + buf.length > SAMPLE_RATE * 2 * MAX_SECONDS) return; const now=Date.now(); if(this.lastAudioAt)this.maxAudioGapMs=Math.max(this.maxAudioGapMs,now-this.lastAudioAt); this.lastAudioAt=now; if(now-this.lastLevelLog>=500){const stats=level(buf);this.log({event:'audio-level',...stats,signal:stats.rms>=120,audioSeconds:(this.bytes+buf.length)/32000,wallSeconds:(now-this.startedAt)/1000});this.lastLevelLog=now;} this.chunks.push(Buffer.from(buf)); this.bytes += buf.length; if (this.live) this.liveAudio(Buffer.from(buf)); else void this.preview(); }
  async stop(reply) {
    if (!this.chunks) return;
    const pcm = Buffer.concat(this.chunks); this.chunks = null;
    this.log({event:'voice-capture-timing',wallSeconds:(Date.now()-this.startedAt)/1000,audioSeconds:pcm.length/32000,maxPacketGapMs:this.maxAudioGapMs});
    if (this.live) {
      const session = this.segment;
      fs.mkdirSync(this.dir, { recursive: true }); fs.writeFileSync(path.join(this.dir, 'last.wav'), wav(pcm));
      reply({type:'voice',state:'recognizing'});
      this.commitSegment(session); await session.queue;
      if (session.generation !== this.generation) return;
      reply(session.error ? {type:'voice',state:'error',text:session.error} : session.text ? {type:'voice',state:'done',text:session.text.slice(-220)} : {type:'voice',state:'error',text:'没听清，再说一次'});
      return;
    }
    const generation = ++this.generation;
    const seconds = pcm.length / 2 / SAMPLE_RATE, stats = level(pcm);
    fs.mkdirSync(this.dir, { recursive: true }); fs.writeFileSync(path.join(this.dir, 'last.wav'), wav(pcm));
    this.log({ event: 'voice', seconds: Number(seconds.toFixed(2)), ...stats, time: new Date().toISOString() });
    if (seconds < MIN_SECONDS) return reply({ type: 'voice', state: 'error', text: '说话时间太短' });
    reply({ type: 'voice', state: 'recognizing' });
    let text;
    try {
      text = (await this.recognize(pcm, { seconds, ...stats })).trim();
    } catch (e) {
      reply({ type: 'voice', state: 'error', text: e.message.includes('语音模型') ? e.message : '语音识别失败，请检查电脑服务日志' });
      this.log({ event: 'voice-error', stage: 'recognize', message: e.message });
      return;
    }
    if (generation !== this.generation) return;
    if (!text) return reply({ type: 'voice', state: 'error', text: '没听清，再说一次' });
    try {
      await this.typeText(text);
      reply({ type: 'voice', state: 'done', text });
    } catch (e) {
      reply({ type: 'voice', state: 'error', text: '已识别，自动输入失败：' + text });
      this.log({ event: 'voice-error', stage: 'type', message: e.message });
    }
  }
}
module.exports = { Voice, wav, level, SAMPLE_RATE };
