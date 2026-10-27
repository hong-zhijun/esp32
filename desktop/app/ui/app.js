// AI 状态屏 desktop UI. A plain ES module that talks to the Rust side through Tauri's global API
// (`withGlobalTauri`). Server-provided text is always written with textContent.
const tauri = window.__TAURI__;

const STATE_NAMES = {
  working: '执行中',
  waiting: '等你确认',
  done: '做完了',
  idle: '空闲',
  interrupted: '已中断',
  unknown: '状态待确认',
};
const PRESETS = {
  'fn-space': { key: 'Space', ctrl: false, alt: false, shift: false, meta: false, fnKey: true },
  f8: { key: 'F8', ctrl: false, alt: false, shift: false, meta: false, fnKey: false },
};
const MODIFIER_CODES = new Set([
  'ShiftLeft', 'ShiftRight', 'ControlLeft', 'ControlRight', 'AltLeft', 'AltRight',
  'MetaLeft', 'MetaRight', 'OSLeft', 'OSRight', 'CapsLock', 'Fn',
]);
const DEBUG_NAMES = {
  'wetype-external-stop': '电脑结束语音',
  'wetype-capture-active': '微信开始采音',
  'wetype-monitor-error': '状态检测异常',
  'virtual-mic-ready': '虚拟麦克风就绪',
  'virtual-mic-drained': '音频已送达',
  'wetype-started': '微信语音启动',
  'wetype-stopped': '微信语音结束',
  'device-performance': '设备耗时',
  button: '按钮请求',
  'recording-start': '开始录音',
  'audio-level': '接收音频',
  'recognition-start': '开始识别',
  recognized: '识别文字',
  'voice-segment-timing': '粘贴完成',
  'voice-capture-timing': '结束录音',
  'voice-error': '错误',
  'device-connected': '屏幕连接',
  'device-disconnected': '屏幕断开',
  'mdns-error': '电脑广播异常',
};
const DEBUG_LIMIT = 300;
const SESSION_LIMIT = 30;
const SAVE_DELAY_MS = 300;
const HOTKEY_TEST_DELAY_MS = 1500;
const isMac = /Mac|iPhone|iPad/.test(navigator.userAgent);
const defaultDeviceName = isMac ? 'BlackHole 2ch' : 'CABLE Input';

const $ = (selector, root = document) => root.querySelector(selector);
const $$ = (selector, root = document) => Array.from(root.querySelectorAll(selector));

let voiceConfig = null;
let pendingVoiceSaves = 0;
let recordingHotkey = false;
let screenDirty = false;
let screenSaveTimer = null;
let screenSaveSeq = 0;
let debugPaused = false;
let lastDebugId = 0;
let noticeTimer = null;
const sessionRows = new Map();

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function invoke(command, args) {
  return tauri.core.invoke(command, args);
}

function errorText(error) {
  return error?.message || String(error);
}

function showNotice(text, duration = 6000) {
  const notice = $('#notice');
  notice.textContent = text;
  notice.hidden = false;
  clearTimeout(noticeTimer);
  if (duration !== Infinity) {
    noticeTimer = setTimeout(() => {
      notice.hidden = true;
    }, duration);
  }
}

/** mm:ss between two epoch-millisecond times, minutes unbounded, as the screen shows it. */
function clock(from, to) {
  const seconds = from && to >= from ? Math.floor((to - from) / 1000) : 0;
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`;
}

function ago(time, now) {
  const seconds = Math.max(0, Math.floor((now - time) / 1000));
  if (seconds < 60) return '刚刚';
  if (seconds < 3600) return `${Math.floor(seconds / 60)} 分钟前`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} 小时前`;
  return `${Math.floor(seconds / 86400)} 天前`;
}

function hotkeyLabel(hotkey) {
  const parts = [];
  if (hotkey.fnKey) parts.push('Fn');
  if (hotkey.ctrl) parts.push('Ctrl');
  if (hotkey.alt) parts.push(isMac ? 'Option' : 'Alt');
  if (hotkey.shift) parts.push('Shift');
  if (hotkey.meta) parts.push(isMac ? 'Command' : 'Win');
  parts.push(hotkey.key === 'Space' ? '空格' : hotkey.key);
  return parts.join('＋');
}

/** Maps a KeyboardEvent.code to a supported shared key name, or null. */
function keyName(code) {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F(1[0-2]|[1-9])$/.test(code)) return code;
  if (code === 'Space' || code === 'Escape' || code === 'Tab') return code;
  if (code === 'Enter' || code === 'NumpadEnter') return 'Enter';
  return null;
}

function showPage(name) {
  for (const page of $$('.page')) page.hidden = page.dataset.page !== name;
  for (const button of $$('.nav-item')) {
    if (button.dataset.page === name) button.setAttribute('aria-current', 'page');
    else button.removeAttribute('aria-current');
  }
  if (name === 'device') refreshDeviceForm();
  if (name === 'settings') loadHooks();
}

// ---- Status page -------------------------------------------------------------------------

function cardDetail(card, screen, now) {
  if (card.state === 'working') {
    const parts = [screen.showTool ? card.tool : '', screen.showElapsed ? clock(card.startedAt, now) : ''];
    return parts.filter(Boolean).join(' · ');
  }
  if (card.state === 'waiting') return `已等 ${clock(card.updatedAt, now)}`;
  if (card.state === 'done') {
    return screen.showElapsed && card.startedAt ? `用时 ${clock(card.startedAt, card.endedAt)}` : '';
  }
  if (card.state === 'unknown') return card.endedAt ? '本轮结束原因需确认' : '5 分钟没有新动静';
  return '';
}

function renderCards(current) {
  for (const card of current.cards) {
    const root = $(`.card[data-provider="${card.provider}"]`);
    if (!root) continue;
    const empty = !card.id;
    root.dataset.state = empty ? 'idle' : card.state;
    $('.card-state', root).textContent = empty ? '空闲' : STATE_NAMES[card.state];
    $('.card-headline', root).textContent = empty ? '暂无会话' : card.title || card.project || card.id;
    $('.card-detail', root).textContent = empty ? '' : cardDetail(card, current.config.screen, current.now);
    $('.card-count', root).textContent = card.count ? `共 ${card.count} 个会话` : '';
    $('.card-pin', root).hidden = empty || current.pinned[card.provider] !== card.id;
  }
}

function createSessionRow(session) {
  const row = el('li', 'session');
  row.dataset.provider = session.provider;
  row.dataset.id = session.id;
  const meta = el('div', 'session-meta');
  meta.append(el('span', 'session-state'), el('span', 'session-time'), el('span', 'session-unread'));
  const main = el('div', 'session-main');
  main.append(el('div', 'session-title'), meta);
  const pin = el('button', '', '固定显示');
  pin.type = 'button';
  pin.dataset.action = 'pin';
  const ack = el('button', '', '清除完成提示');
  ack.type = 'button';
  ack.dataset.action = 'ack';
  row.append(el('span', 'dot'), main, el('div', 'actions'));
  $('.actions', row).append(pin, ack);
  return row;
}

function updateSessionRow(row, session, current) {
  row.dataset.state = session.state;
  $('.session-title', row).textContent = session.title || session.project || session.id;
  $('.session-state', row).textContent = STATE_NAMES[session.state] || session.state;
  $('.session-time', row).textContent = ago(session.updatedAt, current.now);
  $('.session-unread', row).textContent = session.unread ? '待清除' : '';
  const pinned = current.pinned[session.provider] === session.id;
  const pin = $('[data-action="pin"]', row);
  pin.textContent = pinned ? '取消固定' : '固定显示';
  pin.setAttribute('aria-pressed', String(pinned));
  $('[data-action="ack"]', row).hidden = !(session.state === 'done' && session.unread);
}

/** Rows are updated in place so a click is never lost to a rebuild between mousedown and click. */
function renderSessions(current) {
  const list = $('#sessions');
  const sessions = [...current.sessions].sort((a, b) => b.updatedAt - a.updatedAt).slice(0, SESSION_LIMIT);
  const shown = new Set();
  sessions.forEach((session, index) => {
    const key = `${session.provider}:${session.id}`;
    shown.add(key);
    let row = sessionRows.get(key);
    if (!row) {
      row = createSessionRow(session);
      sessionRows.set(key, row);
    }
    updateSessionRow(row, session, current);
    if (list.children[index] !== row) list.insertBefore(row, list.children[index] ?? null);
  });
  for (const [key, row] of sessionRows) {
    if (!shown.has(key)) {
      row.remove();
      sessionRows.delete(key);
    }
  }
  $('#sessionsEmpty').hidden = sessions.length > 0;
}

$('#sessions').addEventListener('click', (event) => {
  const button = event.target.closest('button[data-action]');
  if (!button) return;
  const { provider, id } = button.closest('.session').dataset;
  const request =
    button.dataset.action === 'pin'
      ? invoke('select_session', { provider, id: button.getAttribute('aria-pressed') === 'true' ? '' : id })
      : invoke('ack_session', { provider, id });
  request.catch((error) => showNotice(errorText(error)));
});

// ---- Device page -------------------------------------------------------------------------

function renderDevice(device, now) {
  if (!device) return;
  const status = device.connected
    ? `● 在线${device.ip ? ` · ${device.ip}` : ''}`
    : '○ 离线 · 请检查屏幕是否上电、已连上 Wi‑Fi';
  $('#deviceStatus').textContent = status;
  const parts = [];
  if (device.connected && device.rssi !== null) parts.push(`信号 ${device.rssi} dBm`);
  if (device.fw !== null) parts.push(`固件 ${device.fw}`);
  if (device.ssid) parts.push(`当前 Wi‑Fi ${device.ssid}`);
  if (device.lastSeen) parts.push(`最后通信 ${ago(device.lastSeen, now)}`);
  $('#deviceMeta').textContent = parts.join(' · ');
  renderKnownWifi(device.ssids || []);
}

function renderKnownWifi(ssids) {
  const list = $('#knownWifi');
  const key = JSON.stringify(ssids);
  if (list.dataset.key !== key) {
    list.dataset.key = key;
    list.replaceChildren(
      ...ssids.map((ssid) => {
        const item = el('li');
        const remove = el('button', '', '删除');
        remove.type = 'button';
        remove.dataset.ssid = ssid;
        item.append(el('span', '', ssid), remove);
        return item;
      }),
    );
  }
  $('#knownWifiEmpty').hidden = ssids.length > 0;
}

$('#knownWifi').addEventListener('click', (event) => {
  const button = event.target.closest('button[data-ssid]');
  if (!button) return;
  invoke('forget_wifi', { ssid: button.dataset.ssid }).catch((error) => showNotice(errorText(error)));
});

async function loadPorts() {
  const select = $('#portSelect');
  const current = select.value;
  try {
    const ports = await invoke('list_serial_ports');
    const options = ports.map((port) => new Option(`${port.path} · ${port.label}${port.espressif ? '（屏幕）' : ''}`, port.path));
    if (options.length === 0) options.push(new Option('未找到串口，请用 USB 连接屏幕', ''));
    select.replaceChildren(...options);
    if (ports.some((port) => port.path === current)) select.value = current;
  } catch (error) {
    showNotice(errorText(error));
  }
}

async function loadAddresses() {
  const select = $('#hostSelect');
  const current = select.value;
  try {
    const addresses = await invoke('list_addresses');
    select.replaceChildren(...addresses.map((address) => new Option(address, address)));
    if (addresses.includes(current)) select.value = current;
  } catch (error) {
    showNotice(errorText(error));
  }
}

function refreshDeviceForm() {
  loadPorts();
  loadAddresses();
}

$('#portsRefresh').addEventListener('click', refreshDeviceForm);

$('#provisionForm').addEventListener('submit', async (event) => {
  event.preventDefault();
  const form = event.currentTarget;
  const button = $('#provisionButton');
  const status = $('#provisionStatus');
  const data = new FormData(form);
  const request = {
    port: data.get('port'),
    host: data.get('host'),
    ssid: data.get('ssid'),
    password: data.get('password'),
  };
  button.disabled = true;
  status.textContent = '正在写入，请保持 USB 连接…';
  try {
    await invoke('provision', { request });
    form.elements.password.value = '';
    status.textContent = '已写入！等待屏幕连接 Wi‑Fi，通常需要几秒。';
  } catch (error) {
    status.textContent = errorText(error);
  } finally {
    button.disabled = false;
  }
});

// ---- Voice page --------------------------------------------------------------------------

async function checkVoice() {
  const status = $('#voiceStatus');
  status.textContent = '检查中…';
  try {
    const result = await invoke('voice_status');
    status.textContent = result.message;
    status.dataset.ready = String(result.ready);
  } catch (error) {
    status.textContent = errorText(error);
    status.dataset.ready = 'false';
  }
}

async function loadAudioDevices() {
  try {
    const names = await invoke('list_audio_devices');
    const options = [new Option(`默认（${defaultDeviceName}）`, ''), ...names.map((name) => new Option(name, name))];
    $('#outputDevice').replaceChildren(...options);
    syncOutputDevice();
  } catch (error) {
    showNotice(errorText(error));
  }
}

/** Shows the saved output device, adding it to the list when it is not currently present. */
function syncOutputDevice() {
  const select = $('#outputDevice');
  if (!voiceConfig || document.activeElement === select) return;
  const wanted = voiceConfig.outputDevice ?? '';
  if (!Array.from(select.options).some((option) => option.value === wanted)) {
    select.append(new Option(`${wanted}（未检测到）`, wanted));
  }
  select.value = wanted;
}

function renderVoice(voice) {
  // While a save is in flight the local copy is newer than the snapshot.
  if (pendingVoiceSaves === 0) voiceConfig = voice;
  if (!voiceConfig) return;
  $('#hotkeyLabel').textContent = hotkeyLabel(voiceConfig.hotkey);
  syncOutputDevice();
}

function setHotkeyNote(text) {
  $('#hotkeyNote').textContent = text;
}

async function saveVoice(changes) {
  if (!voiceConfig) return;
  const config = {
    outputDevice: 'outputDevice' in changes ? changes.outputDevice : voiceConfig.outputDevice,
    hotkey: changes.hotkey ?? voiceConfig.hotkey,
  };
  pendingVoiceSaves += 1;
  try {
    await invoke('save_voice_config', { config });
    voiceConfig = config;
    $('#hotkeyLabel').textContent = hotkeyLabel(config.hotkey);
    setHotkeyNote('已保存');
  } catch (error) {
    setHotkeyNote(errorText(error));
  } finally {
    pendingVoiceSaves -= 1;
  }
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

$('#voiceCheck').addEventListener('click', () => {
  checkVoice();
  loadAudioDevices();
});

$('#outputDevice').addEventListener('change', (event) => {
  saveVoice({ outputDevice: event.target.value || null });
});

$('#hotkeyRecord').addEventListener('click', () => {
  recordingHotkey = !recordingHotkey;
  $('#hotkeyRecord').textContent = recordingHotkey ? '取消' : '录制';
  setHotkeyNote(recordingHotkey ? '请按下新的快捷键（Mac 的 Fn 键请用预设）' : '已取消');
});

// Captures the next key while recording; runs before the page sees the event.
window.addEventListener(
  'keydown',
  (event) => {
    if (!recordingHotkey || event.repeat || MODIFIER_CODES.has(event.code)) return;
    event.preventDefault();
    event.stopPropagation();
    const key = keyName(event.code);
    if (!key) {
      setHotkeyNote('不支持这个按键，请换一个');
      return;
    }
    recordingHotkey = false;
    $('#hotkeyRecord').textContent = '录制';
    saveVoice({
      hotkey: {
        key,
        ctrl: event.ctrlKey,
        alt: event.altKey,
        shift: event.shiftKey,
        meta: event.metaKey,
        fnKey: false,
      },
    });
  },
  true,
);

$('#hotkeyPreset').addEventListener('change', (event) => {
  const preset = PRESETS[event.target.value];
  event.target.value = '';
  if (preset) saveVoice({ hotkey: { ...preset } });
});

$('#hotkeyTest').addEventListener('click', async () => {
  if (!voiceConfig) return;
  setHotkeyNote(`${HOTKEY_TEST_DELAY_MS / 1000} 秒后发送一次快捷键，请先切到要输入的窗口`);
  await sleep(HOTKEY_TEST_DELAY_MS);
  try {
    await invoke('test_hotkey', { hotkey: voiceConfig.hotkey });
    setHotkeyNote('已发送一次快捷键');
  } catch (error) {
    setHotkeyNote(errorText(error));
  }
});

function debugFields(entry) {
  const fields = [];
  if (entry.event === 'device-performance') {
    const mode = entry.screenLocked ? '屏幕已锁定' : entry.recording ? '录音中' : '待机';
    fields.push(
      `${mode} · 循环最大 ${entry.loopMs} ms · 采集 ${entry.readMs} ms · 发送 ${entry.sendMs} ms · 读取/发送错误 ${entry.readErrors}/${entry.sendErrors} · Wi-Fi ${entry.rssi} dBm`,
    );
  }
  if (entry.source) fields.push(entry.source);
  if (entry.deviceMs !== undefined) fields.push(`设备运行 ${entry.deviceMs} ms`);
  if (entry.signal !== undefined) fields.push(entry.signal ? '检测到声音信号' : '安静／低音量');
  if (entry.rms !== undefined) fields.push(`音量 RMS ${entry.rms} / 峰值 ${entry.peak}`);
  if (entry.audioSeconds !== undefined) fields.push(`音频 ${entry.audioSeconds.toFixed(2)} s`);
  if (entry.wallSeconds !== undefined) fields.push(`实际经过 ${entry.wallSeconds.toFixed(2)} s`);
  const timings = [
    ['queueMs', '排队'],
    ['recognizeMs', '识别'],
    ['pasteMs', '粘贴'],
    ['maxPacketGapMs', '最大音频间隔'],
  ];
  for (const [key, label] of timings) {
    if (entry[key] !== undefined) fields.push(`${label} ${entry[key]} ms`);
  }
  if (entry.text) fields.push(entry.text);
  if (entry.message) fields.push(entry.message);
  if (entry.stage) fields.push(`阶段 ${entry.stage}`);
  return fields.join(' · ');
}

function debugRow(entry) {
  const date = new Date(entry.time);
  const time = `${date.toLocaleTimeString('zh-CN', { hour12: false })}.${String(date.getMilliseconds()).padStart(3, '0')}`;
  const row = el('div', 'debug-row');
  row.dataset.event = entry.event;
  row.append(
    el('span', 'debug-time', time),
    el('strong', '', DEBUG_NAMES[entry.event] || entry.event),
    el('span', '', debugFields(entry)),
  );
  return row;
}

function appendDebug(entries) {
  const box = $('#debugLog');
  let added = false;
  for (const entry of entries) {
    if (entry.id <= lastDebugId) continue;
    lastDebugId = entry.id;
    box.append(debugRow(entry));
    added = true;
  }
  while (box.childElementCount > DEBUG_LIMIT) box.firstElementChild.remove();
  if (added && !debugPaused) box.scrollTop = box.scrollHeight;
}

$('#debugPause').addEventListener('click', (event) => {
  debugPaused = !debugPaused;
  event.currentTarget.textContent = debugPaused ? '恢复滚动' : '暂停滚动';
});

// Clearing only empties the view; lastDebugId keeps already-shown rows from coming back.
$('#debugClear').addEventListener('click', () => {
  $('#debugLog').replaceChildren();
});

// ---- Settings page -----------------------------------------------------------------------

const screenForm = $('#screenForm');

function updateScreenOutputs() {
  $('#splitValue').textContent = `${screenForm.elements.split.value}%`;
  $('#brightnessValue').textContent = screenForm.elements.brightness.value;
}

function fillScreenForm(screen) {
  const { elements } = screenForm;
  elements.split.value = screen.split;
  elements.brightness.value = screen.brightness;
  elements.claudeColor.value = screen.claudeColor;
  elements.codexColor.value = screen.codexColor;
  elements.deepseekColor.value = screen.deepseekColor || '#7c9fff';
  elements.showTool.checked = screen.showTool;
  elements.showElapsed.checked = screen.showElapsed;
  elements.swap.checked = screen.swap;
  updateScreenOutputs();
}

function readScreenForm() {
  const { elements } = screenForm;
  return {
    split: Number(elements.split.value),
    brightness: Number(elements.brightness.value),
    claudeColor: elements.claudeColor.value,
    codexColor: elements.codexColor.value,
    deepseekColor: elements.deepseekColor.value,
    showTool: elements.showTool.checked,
    showElapsed: elements.showElapsed.checked,
    swap: elements.swap.checked,
  };
}

async function saveScreen() {
  const seq = ++screenSaveSeq;
  try {
    await invoke('save_screen_config', { config: readScreenForm() });
    if (seq === screenSaveSeq) {
      screenDirty = false;
      $('#screenStatus').textContent = '已保存';
    }
  } catch (error) {
    $('#screenStatus').textContent = errorText(error);
  }
}

screenForm.addEventListener('input', () => {
  screenDirty = true;
  updateScreenOutputs();
  $('#screenStatus').textContent = '保存中…';
  clearTimeout(screenSaveTimer);
  screenSaveTimer = setTimeout(saveScreen, SAVE_DELAY_MS);
});

$('#autostart').addEventListener('change', async (event) => {
  const enabled = event.target.checked;
  try {
    await invoke('set_autostart', { enabled });
  } catch (error) {
    event.target.checked = !enabled;
    showNotice(errorText(error));
  }
});

async function loadAutostart() {
  try {
    $('#autostart').checked = await invoke('get_autostart');
  } catch (error) {
    showNotice(errorText(error));
  }
}

function renderHooks(info) {
  const installed = info.installedEvents.length;
  const total = installed + info.missingEvents.length;
  let text =
    installed === total ? `已安装（${installed}/${total} 项）` : `已安装 ${installed}/${total} 项，还缺 ${total - installed} 项`;
  if (info.legacy) text += '，检测到旧版 Node hook，安装后会替换';
  $('#hooksStatus').textContent = text;
  $('#hooksPath').textContent = info.path;
}

async function loadHooks() {
  try {
    renderHooks(await invoke('hooks_status'));
  } catch (error) {
    $('#hooksStatus').textContent = errorText(error);
  }
}

$('#hooksInstall').addEventListener('click', async (event) => {
  const button = event.currentTarget;
  button.disabled = true;
  $('#hooksResult').textContent = '正在写入…';
  try {
    const backup = await invoke('install_hooks');
    $('#hooksResult').textContent = backup ? `已更新，原文件备份为 ${backup}` : '已是最新，无需修改';
    await loadHooks();
  } catch (error) {
    $('#hooksResult').textContent = errorText(error);
  } finally {
    button.disabled = false;
  }
});

function renderServer(server) {
  const text = `服务端口 ${server.port}`;
  $('#serverLine').textContent = server.error ? `${text} · ${server.error}` : text;
}

// ---- Wiring ------------------------------------------------------------------------------

function applySnapshot(next) {
  renderCards(next);
  renderSessions(next);
  renderDevice(next.device, next.now);
  renderVoice(next.config.voice);
  if (!screenDirty) fillScreenForm(next.config.screen);
  renderServer(next.server);
  const sourceStatus = $('#sourceStatus');
  const sourceErrors = next.sourceErrors || [];
  sourceStatus.hidden = sourceErrors.length === 0;
  sourceStatus.textContent = sourceErrors.map(code =>
    code.startsWith('deepseek_') ? `DeepSeek 会话暂时无法读取（${code}）` : `会话来源异常（${code}）`
  ).join('；');
}

for (const button of $$('.nav-item')) {
  button.addEventListener('click', () => showPage(button.dataset.page));
}

async function start() {
  if (!tauri?.core || !tauri?.event) {
    showNotice('请在 AI 状态屏桌面程序中打开此界面。', Infinity);
    return;
  }
  tauri.event.listen('snapshot', (event) => applySnapshot(event.payload));
  tauri.event.listen('device', (event) => renderDevice(event.payload, Date.now()));
  tauri.event.listen('debug', (event) => appendDebug([event.payload]));
  try {
    applySnapshot(await invoke('get_snapshot'));
    appendDebug(await invoke('get_debug_log'));
  } catch (error) {
    showNotice(errorText(error));
  }
  loadAutostart();
  loadHooks();
  checkVoice();
  loadAudioDevices();
  refreshDeviceForm();
}

start();
