// Ferrite desktop shell. The Rust side owns the server; this file only renders its state.
(() => {
  const $ = (selector) => document.querySelector(selector);
  const tauri = window.__TAURI__;
  const invoke = tauri?.core?.invoke ?? (async () => null);
  const listen = tauri?.event?.listen ?? (async () => () => {});
  const appWindow = tauri?.window?.getCurrentWindow?.();

  const el = {
    start: $('#startButton'),
    stop: $('#stopButton'),
    commandForm: $('#commandForm'),
    command: $('#commandInput'),
    send: $('#sendButton'),
    console: $('#console'),
    welcome: $('#welcome'),
    autoScroll: $('#autoScrollButton'),
    clear: $('#clearButton'),
    statusLabel: $('#statusLabel'),
    statusHeading: $('#statusHeading'),
    statusDetail: $('#statusDetail'),
    uptime: $('#uptime'),
    consoleSub: $('#consoleSub'),
    consoleStatus: $('#consoleStatus'),
    address: $('#addressValue'),
    version: $('#versionValue'),
    protocol: $('#protocolValue'),
    titleVersion: $('#titleVersion'),
    footerHint: $('#footerHint'),
    toast: $('#toast'),
  };

  const phases = {
    stopped: {
      label: '중지됨',
      heading: '서버가 중지되어 있습니다',
      detail: '시작 버튼을 눌러 월드와 플러그인을 불러오세요.',
      console: '대기 중',
      sub: '서버 출력이 여기에 표시됩니다',
    },
    starting: {
      label: '시작 중',
      heading: 'Ferrite를 깨우는 중입니다',
      detail: '월드, 플러그인, 월드젠 호스트를 준비하고 있습니다.',
      console: '시작 중',
      sub: '서버를 준비하고 있습니다',
    },
    running: {
      label: '실행 중',
      heading: '서버가 실행 중입니다',
      detail: '플레이어를 기다리고 있습니다. 콘솔에서 명령을 실행할 수 있습니다.',
      console: '연결됨',
      sub: '실시간 서버 출력',
    },
    stopping: {
      label: '종료 중',
      heading: '안전하게 저장하는 중입니다',
      detail: '월드와 플레이어 데이터를 저장한 뒤 창을 닫습니다.',
      console: '종료 중',
      sub: '저장 후 종료합니다',
    },
  };

  const state = {
    phase: 'stopped',
    autoscroll: true,
    startedAt: null,
    history: [],
    historyIndex: -1,
    draft: '',
    toastTimer: null,
    uptimeTimer: null,
  };
  const welcome = el.welcome?.outerHTML ?? '';
  const maxLines = 2500;

  function setPhase(phase) {
    const copy = phases[phase] ?? phases.stopped;
    state.phase = phase;
    document.body.dataset.phase = phase;
    el.statusLabel.textContent = copy.label;
    el.statusHeading.textContent = copy.heading;
    el.statusDetail.textContent = copy.detail;
    el.consoleSub.textContent = copy.sub;
    el.consoleStatus.innerHTML = `<span class="foot-dot"></span>${copy.console}`;

    const stopped = phase === 'stopped';
    const running = phase === 'running';
    el.start.disabled = !stopped;
    el.stop.disabled = stopped || phase === 'stopping';
    el.command.disabled = !running;
    el.send.disabled = !running;

    if (running) {
      if (!state.startedAt) state.startedAt = Date.now();
      el.command.focus();
    } else {
      state.startedAt = null;
      el.uptime.textContent = '00:00:00';
    }
    refreshUptime();
  }

  function refreshUptime() {
    if (!state.startedAt) return;
    const elapsed = Math.max(0, Math.floor((Date.now() - state.startedAt) / 1000));
    const hours = String(Math.floor(elapsed / 3600)).padStart(2, '0');
    const minutes = String(Math.floor((elapsed % 3600) / 60)).padStart(2, '0');
    const seconds = String(elapsed % 60).padStart(2, '0');
    el.uptime.textContent = `${hours}:${minutes}:${seconds}`;
  }

  function errorText(error) {
    return typeof error === 'string' ? error : error?.message ?? String(error);
  }

  function toast(message) {
    el.toast.textContent = message;
    el.toast.classList.add('show');
    clearTimeout(state.toastTimer);
    state.toastTimer = setTimeout(() => el.toast.classList.remove('show'), 4200);
  }

  function classify(line) {
    const lower = line.toLowerCase();
    if (line.startsWith('›')) return 'command';
    if (lower.includes(' error ') || lower.startsWith('error') || lower.includes('failed')) return 'error';
    if (lower.includes(' warn ') || lower.startsWith('warn')) return 'warn';
    if (lower.includes(' info ') || lower.startsWith('info')) return 'info';
    return '';
  }

  function appendLog(payload) {
    const text = String(payload ?? '').replace(/\u001b\[[0-9;]*m/g, '');
    if (!text) return;
    el.welcome?.remove();
    const stick = state.autoscroll;
    for (const line of text.split(/\r?\n/)) {
      if (!line) continue;
      const row = document.createElement('div');
      row.className = `line ${classify(line)}`.trim();
      row.textContent = line;
      el.console.appendChild(row);
    }
    while (el.console.children.length > maxLines) {
      el.console.firstElementChild?.remove();
    }
    if (stick) {
      requestAnimationFrame(() => {
        el.console.scrollTop = el.console.scrollHeight;
      });
    }
  }

  function clearLog() {
    el.console.innerHTML = welcome;
    el.welcome = $('#welcome');
    el.console.scrollTop = 0;
    el.command.focus();
  }

  async function start() {
    if (state.phase !== 'stopped') return;
    setPhase('starting');
    try {
      await invoke('start_server');
    } catch (error) {
      setPhase('stopped');
      toast(errorText(error));
    }
  }

  async function stop() {
    if (state.phase !== 'running' && state.phase !== 'starting') return;
    setPhase('stopping');
    try {
      await invoke('stop_server');
    } catch (error) {
      setPhase('running');
      toast(errorText(error));
    }
  }

  async function sendCommand() {
    const command = el.command.value.trim();
    if (!command || state.phase !== 'running') return;
    if (state.history.at(-1) !== command) {
      state.history.push(command);
      if (state.history.length > 200) state.history.shift();
    }
    state.historyIndex = -1;
    state.draft = '';
    appendLog(`› ${command}`);
    el.command.value = '';
    el.command.focus();
    try {
      await invoke('send_command', { command });
    } catch (error) {
      toast(errorText(error));
    }
  }

  function showHistory(direction) {
    if (!state.history.length) return;
    if (direction < 0) {
      if (state.historyIndex === -1) {
        state.draft = el.command.value;
        state.historyIndex = state.history.length - 1;
      } else {
        state.historyIndex = Math.max(0, state.historyIndex - 1);
      }
      el.command.value = state.history[state.historyIndex] ?? '';
    } else {
      if (state.historyIndex === -1) return;
      state.historyIndex += 1;
      if (state.historyIndex >= state.history.length) {
        state.historyIndex = -1;
        el.command.value = state.draft;
      } else {
        el.command.value = state.history[state.historyIndex] ?? '';
      }
    }
    el.command.setSelectionRange(el.command.value.length, el.command.value.length);
  }

  async function closeWindow() {
    try {
      await invoke('request_close');
    } catch (error) {
      toast(errorText(error));
      appWindow?.close?.();
    }
  }

  el.start.addEventListener('click', start);
  el.stop.addEventListener('click', stop);
  el.commandForm.addEventListener('submit', (event) => {
    event.preventDefault();
    sendCommand();
  });
  el.clear.addEventListener('click', clearLog);
  el.autoScroll.addEventListener('click', () => {
    state.autoscroll = !state.autoscroll;
    el.autoScroll.classList.toggle('active', state.autoscroll);
    el.autoScroll.setAttribute('aria-pressed', String(state.autoscroll));
    if (state.autoscroll) el.console.scrollTop = el.console.scrollHeight;
  });

  $('#minimizeButton').addEventListener('click', () => appWindow?.minimize?.());
  $('#maximizeButton').addEventListener('click', () => appWindow?.toggleMaximize?.());
  $('#closeButton').addEventListener('click', closeWindow);

  const titlebar = document.querySelector('.titlebar');
  titlebar.addEventListener('dblclick', (event) => {
    if (!event.target.closest('.window-actions')) appWindow?.toggleMaximize?.();
  });

  document.addEventListener('keydown', (event) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'l') {
      event.preventDefault();
      clearLog();
    }
  });

  el.command.addEventListener('keydown', (event) => {
    if (event.key === 'ArrowUp') {
      event.preventDefault();
      showHistory(-1);
    } else if (event.key === 'ArrowDown') {
      event.preventDefault();
      showHistory(1);
    }
  });

  listen('server-log', (event) => appendLog(event.payload)).catch(() => {});
  listen('server-started', () => setPhase('running')).catch(() => {});
  listen('server-stopped', () => {
    setPhase('stopped');
    appendLog('서버가 종료되었습니다. 콘솔은 새 세션을 기다립니다.');
  }).catch(() => {});

  state.uptimeTimer = setInterval(refreshUptime, 1000);
  invoke('server_info')
    .then((info) => {
      if (!info) return;
      el.address.textContent = info.address;
      el.version.textContent = info.version;
      el.protocol.textContent = String(info.protocol);
      el.titleVersion.textContent = `v${info.version}`;
      el.footerHint.textContent = `Minecraft Java 26.3 · protocol ${info.protocol}`;
    })
    .catch(() => {});

  setPhase('stopped');
})();
