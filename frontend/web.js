// 浏览器里的 Tauri 替身：页面仍调用 invoke / listen，请求打到本机服务。
(function () {
  window.__LOCMODEL_WEB__ = true;
  const listeners = new Map();
  let source = null;
  let sourceStarting = false;
  let authReady = null;
  let token = '';

  function authHeaders(extra) {
    const headers = Object.assign({}, extra || {});
    if (token) headers.Authorization = 'Bearer ' + token;
    return headers;
  }
  window.locmodelAuthHeaders = authHeaders;

  function showLogin() {
    return new Promise((resolve) => {
      const old = document.getElementById('web-login');
      if (old) old.remove();
      const box = document.createElement('div');
      box.id = 'web-login';
      box.innerHTML = '<form class="web-card"><h2>连接 LocModel 服务器</h2><p>口令在服务器启动日志里，也写在数据目录的 access-token.txt。</p><input id="web-token" type="password" autocomplete="current-password" placeholder="访问口令"><p class="web-err" id="web-login-err"></p><button type="submit">进入</button></form>';
      document.body.appendChild(box);
      const input = box.querySelector('#web-token');
      input.focus();
      box.querySelector('form').addEventListener('submit', async (ev) => {
        ev.preventDefault();
        const next = input.value.trim();
        const res = await fetch('/api/session', { headers: { Authorization: 'Bearer ' + next } });
        const body = await res.json().catch(() => ({}));
        if (!body.ok) {
          box.querySelector('#web-login-err').textContent = '口令不正确';
          return;
        }
        token = next;
        try { sessionStorage.setItem('locmodel-token', token); } catch {}
        box.remove();
        resolve(token);
      });
    });
  }

  function ensureAuth() {
    if (!authReady) {
      authReady = (async () => {
        const first = await fetch('/api/session').then((r) => r.json());
        if (!first.required) return '';
        try { token = sessionStorage.getItem('locmodel-token') || ''; } catch { token = ''; }
        if (token) {
          const check = await fetch('/api/session', { headers: authHeaders() }).then((r) => r.json());
          if (check.ok) return token;
          token = '';
          try { sessionStorage.removeItem('locmodel-token'); } catch {}
        }
        return showLogin();
      })();
    }
    return authReady;
  }

  async function rawInvoke(cmd, args) {
    await ensureAuth();
    const res = await fetch('/api/invoke', {
      method: 'POST',
      headers: authHeaders({ 'Content-Type': 'application/json' }),
      body: JSON.stringify({ cmd, args: args || {} }),
    });
    const body = await res.json().catch(() => ({ ok: false, error: '服务没有返回 JSON' }));
    if (!res.ok || body.ok === false) throw new Error(body.error || ('请求失败 ' + res.status));
    return body.value;
  }

  // 网页版：服务器上文件的直接地址（视频播放用，支持流式加载）
  window.locmodelRawUrl = (path) => {
    const q = new URLSearchParams({ path });
    if (token) q.set('token', token);
    return '/api/raw?' + q.toString();
  };

  function downloadFile(path) {
    const q = new URLSearchParams({ path });
    if (token) q.set('token', token);
    const a = document.createElement('a');
    a.href = '/api/raw?' + q.toString();
    a.download = '';
    document.body.appendChild(a);
    a.click();
    a.remove();
  }

  function selectedFiles(panel) {
    try { return JSON.parse(panel.dataset.files || '[]'); } catch { return []; }
  }

  function joinPath(dir, name) {
    const sep = String(dir).includes('\\') ? '\\' : '/';
    return String(dir).replace(/[\\/]+$/, '') + sep + name;
  }

  async function uploadFiles(list) {
    const fd = new FormData();
    for (const file of list) fd.append('file', file, file.webkitRelativePath || file.name);
    const res = await fetch('/api/upload', { method: 'POST', headers: authHeaders(), body: fd });
    const body = await res.json().catch(() => ({ ok: false, error: '上传失败' }));
    if (!res.ok || body.ok === false) throw new Error(body.error || '上传失败');
    return body;
  }

  function dispatch(name, payload) {
    const set = listeners.get(name);
    if (!set) return;
    for (const fn of [...set]) {
      try { fn({ payload }); } catch (err) { console.error(err); }
    }
  }

  function startEvents() {
    if (source || sourceStarting) return;
    sourceStarting = true;
    ensureAuth().then(() => {
      const q = token ? ('?token=' + encodeURIComponent(token)) : '';
      source = new EventSource('/api/events' + q);
      source.onmessage = (ev) => {
        let msg;
        try { msg = JSON.parse(ev.data); } catch { return; }
        dispatch(msg.event, msg.payload);
      };
      source.onerror = () => {
        source?.close();
        source = null;
        sourceStarting = false;
        setTimeout(startEvents, 1000);
      };
    });
  }

  function listen(name, cb) {
    if (!listeners.has(name)) listeners.set(name, new Set());
    listeners.get(name).add(cb);
    startEvents();
    return () => listeners.get(name)?.delete(cb);
  }

  const style = document.createElement('style');
  style.textContent = '#web-login,#web-dialog{position:fixed;inset:0;z-index:10000;background:rgba(0,0,0,.45);display:flex;align-items:center;justify-content:center;padding:24px}.web-card{width:min(720px,100%);max-height:min(80vh,760px);overflow:auto;background:#fff;color:#1c1c1c;border-radius:16px;padding:20px;box-shadow:0 16px 50px rgba(0,0,0,.2)}html[data-theme=dark] .web-card{background:#1e1f22;color:#f2f2f2}.web-card h2{margin:0 0 8px;font-size:18px}.web-card p{margin:0 0 12px;color:#666;font-size:13px}html[data-theme=dark] .web-card p{color:#aaa}.web-card input,.web-card button,.web-row{font:inherit}.web-card input{width:100%;box-sizing:border-box;padding:8px 10px;border:1px solid #ccc;border-radius:8px;background:transparent;color:inherit}.web-actions{display:flex;flex-wrap:wrap;gap:8px;margin-top:12px}.web-card button,.web-actions label{border:0;border-radius:8px;padding:8px 12px;background:#2160d0;color:#fff;cursor:pointer}.web-card button.secondary{background:#e8e8e8;color:#222}html[data-theme=dark] .web-card button.secondary{background:#333;color:#eee}.web-list{border:1px solid #ddd;border-radius:10px;margin-top:12px;max-height:360px;overflow:auto}html[data-theme=dark] .web-list{border-color:#333}.web-row{display:flex;justify-content:space-between;gap:12px;padding:8px 10px;cursor:pointer}.web-row:hover,.web-row.on{background:rgba(33,96,208,.12)}.web-err{color:#c0392b;min-height:1.2em;font-size:13px}@media(max-width:760px){#web-login,#web-dialog{padding:0;align-items:flex-end}.web-card{width:100%;max-height:min(92dvh,760px);border-radius:16px 16px 0 0;padding:16px 16px calc(16px + env(safe-area-inset-bottom))}.web-card input{font-size:16px}.web-list{max-height:46dvh}.web-actions{position:sticky;bottom:0;background:inherit;padding-top:8px}}';
  document.head.appendChild(style);

  function openPanel(title, hint) {
    const old = document.getElementById('web-dialog');
    if (old) old.remove();
    const root = document.createElement('div');
    root.id = 'web-dialog';
    root.innerHTML = '<div class="web-card"><h2></h2><p></p><input class="web-path" placeholder="服务器上的绝对路径"><div class="web-list"></div><p class="web-err"></p><div class="web-actions"></div></div>';
    root.querySelector('h2').textContent = title;
    root.querySelector('p').textContent = hint;
    document.body.appendChild(root);
    return root;
  }

  async function renderDir(panel, path, pick) {
    const list = panel.querySelector('.web-list');
    const err = panel.querySelector('.web-err');
    err.textContent = '';
    list.innerHTML = '';
    let data;
    try {
      data = await rawInvoke('fs_list', { path: path || '' });
    } catch (e) {
      err.textContent = String(e.message || e);
      return;
    }
    panel.querySelector('.web-path').value = data.path || '';
    if (data.parent) {
      const up = document.createElement('div');
      up.className = 'web-row';
      up.textContent = '上级目录';
      up.onclick = () => renderDir(panel, data.parent, pick);
      list.appendChild(up);
    }
    for (const entry of data.entries || []) {
      const row = document.createElement('div');
      row.className = 'web-row';
      const name = document.createElement('span');
      name.textContent = (entry.dir ? '文件夹  ' : '文件  ') + entry.name;
      row.appendChild(name);
      if (!entry.dir && entry.size) {
        const size = document.createElement('span');
        size.textContent = Math.ceil(entry.size / 1024) + ' KB';
        row.appendChild(size);
      }
      row.onclick = () => {
        if (entry.dir) {
          renderDir(panel, entry.path, pick);
          return;
        }
        if (pick && pick.extensions && pick.extensions.length) {
          const lower = String(entry.name).toLowerCase();
          const ok = pick.extensions.some((ext) => lower.endsWith('.' + String(ext).toLowerCase()));
          if (!ok) {
            err.textContent = '这个文件类型不符合当前操作';
            return;
          }
        }
        if (pick && pick.kind === 'files') {
          const chosen = selectedFiles(panel);
          const at = chosen.indexOf(entry.path);
          if (at >= 0) chosen.splice(at, 1);
          else chosen.push(entry.path);
          panel.dataset.files = JSON.stringify(chosen);
          row.classList.toggle('on', at < 0);
        } else {
          panel.querySelectorAll('.web-row').forEach((el) => el.classList.remove('on'));
          row.classList.add('on');
          panel.dataset.file = entry.path;
        }
      };
      row.ondblclick = () => {
        if (!entry.dir && pick && pick.kind === 'file') pick.finish([entry.path]);
      };
      list.appendChild(row);
    }
  }

  function addButton(actions, label, className, onClick) {
    const button = document.createElement('button');
    button.type = 'button';
    button.textContent = label;
    if (className) button.className = className;
    button.onclick = onClick;
    actions.appendChild(button);
    return button;
  }

  function addUpload(actions, label, directory, multiple, onFiles) {
    const input = document.createElement('input');
    input.type = 'file';
    input.hidden = true;
    if (directory) input.webkitdirectory = true;
    if (multiple) input.multiple = true;
    input.onchange = () => onFiles(input.files);
    const mark = document.createElement('label');
    mark.textContent = label;
    mark.appendChild(input);
    mark.onclick = (ev) => {
      if (ev.target === input) return;
    };
    actions.appendChild(mark);
  }

  async function resolveDialog(id, paths) {
    await ensureAuth();
    await fetch('/api/dialog', {
      method: 'POST',
      headers: authHeaders({ 'Content-Type': 'application/json' }),
      body: JSON.stringify({ id, paths: paths || [] }),
    });
    document.getElementById('web-dialog')?.remove();
  }

  function showDialog(payload) {
    const kind = payload.kind;
    const title = { folder: '选择服务器上的文件夹', file: '选择服务器上的文件', files: '选择服务器上的文件', save: '保存到服务器' }[kind] || '选择';
    const hint = kind === 'save'
      ? '可以保存到服务器目录，也可以下载到你正在用的这台电脑。'
      : '可以浏览服务器磁盘，也可以从你正在用的电脑上传。大模型建议先放到服务器再选。';
    const panel = openPanel(title, hint);
    const actions = panel.querySelector('.web-actions');
    const pathInput = panel.querySelector('.web-path');
    const finish = (paths) => resolveDialog(payload.id, paths);
    panel.dataset.file = '';
    renderDir(panel, payload.dataDir || '', { kind, extensions: payload.extensions || [], finish });
    pathInput.addEventListener('change', () => renderDir(panel, pathInput.value.trim(), { kind, extensions: payload.extensions || [], finish }));
    if (kind === 'folder') {
      addButton(actions, '使用当前文件夹', '', () => finish([pathInput.value.trim() || payload.dataDir]));
      addUpload(actions, '上传文件夹', true, true, async (files) => {
        try {
          const uploaded = await uploadFiles(files);
          finish([uploaded.dir]);
        } catch (err) {
          panel.querySelector('.web-err').textContent = String(err.message || err);
        }
      });
    } else if (kind === 'save') {
      const name = document.createElement('input');
      name.value = payload.fileName || 'export.txt';
      name.placeholder = '文件名';
      actions.parentNode.insertBefore(name, actions);
      addButton(actions, '保存到当前文件夹', '', () => finish([joinPath(pathInput.value.trim() || payload.dataDir, name.value.trim())]));
      addButton(actions, '下载到我的电脑', 'secondary', () => finish([joinPath(joinPath(payload.dataDir, 'exports'), name.value.trim() || 'export.txt')]));
    } else {
      addButton(actions, '使用选中的文件', '', () => {
        const files = kind === 'files' ? selectedFiles(panel) : (panel.dataset.file ? [panel.dataset.file] : []);
        if (!files.length) {
          panel.querySelector('.web-err').textContent = '先点一个文件';
          return;
        }
        finish(files);
      });
      addUpload(actions, kind === 'files' ? '上传多个文件' : '上传文件', false, kind === 'files', async (files) => {
        try {
          const uploaded = await uploadFiles(files);
          finish(uploaded.paths || []);
        } catch (err) {
          panel.querySelector('.web-err').textContent = String(err.message || err);
        }
      });
    }
    addButton(actions, '取消', 'secondary', () => finish([]));
  }

  function showBrowser(path) {
    const panel = openPanel('服务器上的文件', path || '');
    const actions = panel.querySelector('.web-actions');
    renderDir(panel, path || '', null);
    panel.querySelector('.web-path').addEventListener('change', () => renderDir(panel, panel.querySelector('.web-path').value.trim(), null));
    addButton(actions, '下载选中的文件', '', () => {
      if (panel.dataset.file) downloadFile(panel.dataset.file);
    });
    addButton(actions, '关闭', 'secondary', () => panel.remove());
  }

  async function invoke(cmd, args) {
    if (cmd === 'open_url') {
      const url = await rawInvoke(cmd, args);
      if (url) window.open(url, '_blank', 'noopener');
      return null;
    }
    const value = await rawInvoke(cmd, args);
    if (cmd === 'open_path' || cmd === 'open_images_dir') {
      const path = value && typeof value === 'object' ? value.path : value;
      if (value && value.dir) showBrowser(path);
      else if (path) downloadFile(path);
    }
    if (cmd === 'save_text_file' || cmd === 'save_image_file' || cmd === 'install_update') {
      const path = value && typeof value === 'object' ? value.path : value;
      if (typeof path === 'string' && path) downloadFile(path);
    }
    return value;
  }

  window.__TAURI__ = {
    core: { invoke },
    event: { listen, emit: async () => {} },
    app: { getVersion: async () => '3.1.0-web' },
    window: {
      getCurrentWindow: () => ({
        isFullscreen: async () => !!document.fullscreenElement,
        setFullscreen: async (on) => {
          if (on) await document.documentElement.requestFullscreen();
          else if (document.fullscreenElement) await document.exitFullscreen();
        },
      }),
    },
  };

  listen('web-dialog', (ev) => showDialog(ev.payload || {}));

  window.addEventListener('dragenter', (ev) => {
    if (![...ev.dataTransfer?.types || []].includes('Files')) return;
    ev.preventDefault();
    document.body.classList.add('dragging');
    dispatch('tauri://drag-enter', {});
  });
  window.addEventListener('dragover', (ev) => {
    if (![...ev.dataTransfer?.types || []].includes('Files')) return;
    ev.preventDefault();
  });
  window.addEventListener('dragleave', (ev) => {
    if (ev.target !== document.documentElement && ev.target !== document.body) return;
    document.body.classList.remove('dragging');
    dispatch('tauri://drag-leave', {});
  });
  const narrow = () => window.matchMedia('(max-width: 760px)').matches;
  function pinNav() {
    if (!narrow()) return;
    document.querySelector('.nav button.on')?.scrollIntoView({ inline: 'center', block: 'nearest' });
  }
  const nav = document.querySelector('.nav');
  if (nav) new MutationObserver(pinNav).observe(nav, { subtree: true, attributes: true, attributeFilter: ['class'] });
  document.addEventListener('click', (ev) => {
    if (ev.target.closest('.nav button')) requestAnimationFrame(pinNav);
    if (!narrow() || !ev.target.closest('.chat-wrap') || ev.target.closest('.ag-wrap')) return;
    if (ev.target.closest('.chat-list, .chat-rail, [data-act="toggleChatList"]')) return;
    const open = document.querySelector('.chat-wrap:not(.ag-wrap) .chat-list');
    if (!open) return;
    ev.preventDefault();
    ev.stopPropagation();
    open.querySelector('[data-act="toggleChatList"]')?.click();
  }, true);
  window.addEventListener('resize', pinNav);

  window.addEventListener('drop', async (ev) => {
    if (!ev.dataTransfer?.files?.length) return;
    ev.preventDefault();
    document.body.classList.remove('dragging');
    try {
      const uploaded = await uploadFiles(ev.dataTransfer.files);
      dispatch('tauri://drag-drop', {
        paths: uploaded.paths || [],
        position: { x: ev.clientX, y: ev.clientY },
      });
    } catch (err) {
      console.error(err);
    }
  });
})();
