const path = require('node:path');
const fs = require('node:fs');
// Offline SenseVoice (Chinese/English) via sherpa-onnx; loaded lazily so the panel still runs without the model.
const MODEL_DIR = path.join(__dirname, 'models', 'sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09');
let recognizer = null;
function load() {
  if (recognizer) return recognizer;
  if (!['model.int8.onnx', 'tokens.txt'].every(file => fs.existsSync(path.join(MODEL_DIR, file)))) throw Error('语音模型缺失，请在电脑端安装模型');
  const sherpa = require('sherpa-onnx-node');
  recognizer = new sherpa.OfflineRecognizer({
    featConfig: { sampleRate: 16000, featureDim: 80 },
    modelConfig: { senseVoice: { model: path.join(MODEL_DIR, 'model.int8.onnx'), language: 'auto', useInverseTextNormalization: 1 }, tokens: path.join(MODEL_DIR, 'tokens.txt'), numThreads: 2, provider: 'cpu', debug: 0 },
  });
  return recognizer;
}
async function decode(pcm) {
  const r = load(), samples = new Float32Array(pcm.length >> 1);
  for (let i = 0; i < samples.length; i++) samples[i] = pcm.readInt16LE(i * 2) / 32768;
  const stream = r.createStream();
  stream.acceptWaveform({ sampleRate: 16000, samples });
  const result = await r.decodeAsync(stream);
  return result.text.trim();
}
// Serialize native recognizer access while leaving the Node event loop free.
let pending = Promise.resolve();
function recognize(pcm) {
  const result = pending.then(() => decode(pcm));
  pending = result.catch(() => {});
  return result;
}
module.exports = { recognize, load };
