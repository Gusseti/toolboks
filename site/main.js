(() => {
  const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const css = getComputedStyle(document.documentElement);
  const tok = n => css.getPropertyValue(n).trim();
  const C = { accent: tok('--accent'), mem: tok('--mem'), gpu: tok('--gpu'), disk: tok('--disk'), net: tok('--net'), line: tok('--line-soft'), muted: tok('--muted') };

  // ----- series -----
  const walk = (n, base, spread, lo, hi, spiky) => {
    const a = []; let v = base;
    for (let i = 0; i < n; i++) { v = next(v, base, spread, lo, hi, spiky); a.push(v); }
    return a;
  };
  function next(v, base, spread, lo, hi, spiky) {
    v += (Math.random() - .5) * spread + (base - v) * .12;
    if (spiky && Math.random() < .08) v += spread * 2.5 * Math.random();
    return Math.max(lo, Math.min(hi, v));
  }
  const S = [
    { d: walk(90, 44, 10, 8, 96), base: 44, spread: 10, lo: 8, hi: 96, max: 100, color: C.accent, fmt: v => Math.round(v) + '%' },
    { d: walk(90, 60, 1.2, 55, 66), base: 60, spread: 1.2, lo: 55, hi: 66, max: 100, color: C.mem, fmt: v => Math.round(v) + '%' },
    { d: walk(90, 13, 5, 2, 70), base: 13, spread: 5, lo: 2, hi: 70, max: 100, color: C.gpu, fmt: v => Math.round(v) + '%' },
    { d: walk(90, 4, 3, 0, 40, true), base: 4, spread: 3, lo: 0, hi: 40, max: 40, color: C.disk, fmt: v => v.toFixed(1) + ' MB/s' },
    { d: walk(90, 3, 3, 0, 30, true), base: 3, spread: 3, lo: 0, hi: 30, max: 30, color: C.net, fmt: v => Math.round(v) + ' KB/s' },
  ];
  const coreS = Array.from({ length: 4 }, () => ({ d: walk(60, 45, 9, 5, 95), base: 45, spread: 9, lo: 5, hi: 95, max: 100 }));

  function fit(cv) {
    const r = cv.getBoundingClientRect(), dpr = Math.min(devicePixelRatio || 1, 2);
    const w = Math.max(1, Math.round(r.width * dpr)), h = Math.max(1, Math.round(r.height * dpr));
    if (cv.width !== w || cv.height !== h) { cv.width = w; cv.height = h; }
    const ctx = cv.getContext('2d'); ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    return { ctx, w: r.width, h: r.height };
  }
  function draw(cv, data, max, color, opt = {}) {
    const { ctx, w, h } = fit(cv);
    ctx.clearRect(0, 0, w, h);
    const top = opt.top ?? 4, bot = opt.bot ?? 2, shift = opt.shift ?? 0;
    const n = data.length, step = w / (n - 2);
    const y = v => top + (1 - v / max) * (h - top - bot);
    if (opt.grid) {
      ctx.strokeStyle = C.line; ctx.lineWidth = 1;
      [0.5, 1].forEach(f => { const yy = Math.round(y(max * f)) + .5; ctx.beginPath(); ctx.moveTo(0, yy); ctx.lineTo(w, yy); ctx.stroke(); });
    }
    ctx.beginPath();
    for (let i = 0; i < n; i++) { const x = i * step - shift * step; i ? ctx.lineTo(x, y(data[i])) : ctx.moveTo(x, y(data[i])); }
    ctx.strokeStyle = color; ctx.lineWidth = opt.lw ?? 1.5; ctx.lineJoin = 'round'; ctx.stroke();
    if (opt.fill !== false) {
      ctx.lineTo((n - 1) * step - shift * step, h); ctx.lineTo(-shift * step, h); ctx.closePath();
      ctx.fillStyle = color + (opt.alpha ?? '1f'); ctx.fill();
    }
    return { y, step };
  }

  // ----- window list -----
  const names = [['CPU', 'i7, 4.8 GHz'], ['Memory', '19.0 of 31.9 GB'], ['GPU', 'RTX 3080, 57 °C'], ['Disk', 'Write 4.4 MB/s'], ['Network', 'Sending 1 KB/s']];
  const list = document.getElementById('winList');
  names.forEach(([n, s], i) => {
    const el = document.createElement('div'); el.className = 'res' + (i === 0 ? ' on' : '');
    el.innerHTML = `<canvas></canvas><div><b>${n}</b><small>${s}</small></div><span class="v"></span>`;
    list.appendChild(el);
  });
  const coresEl = document.getElementById('cores');
  coreS.forEach((c, i) => { const g = document.createElement('div'); g.className = 'gcard'; g.innerHTML = `<span>Core ${i}<b></b></span><canvas></canvas>`; coresEl.appendChild(g); });

  function renderWindow() {
    list.querySelectorAll('.res').forEach((el, i) => {
      const s = S[i]; draw(el.querySelector('canvas'), s.d.slice(-30), s.max, s.color, { fill: false, lw: 1.3, top: 2 });
      el.querySelector('.v').textContent = s.fmt(s.d[s.d.length - 1]);
    });
    draw(document.getElementById('cpuBig'), S[0].d, 100, C.accent, { grid: true, top: 10, alpha: '24' });
    coresEl.querySelectorAll('.gcard').forEach((g, i) => {
      const c = coreS[i]; draw(g.querySelector('canvas'), c.d, 100, C.accent, { top: 26, alpha: '24', lw: 1.2 });
      g.querySelector('b').textContent = Math.round(c.d[c.d.length - 1]) + '%';
    });
    document.getElementById('stCpu').textContent = S[0].fmt(S[0].d.at(-1));
    document.getElementById('stRam').textContent = S[1].fmt(S[1].d.at(-1));
    document.getElementById('stGpu').textContent = S[2].fmt(S[2].d.at(-1));
  }
  const rcards = [...document.querySelectorAll('.rcard canvas')];
  function renderCards() {
    rcards.forEach(cv => { const s = S[+cv.dataset.series]; draw(cv, s.d.slice(-40), s.max, s.color, { top: 30, bot: 22, alpha: '22' }); });
    [0, 1, 2, 3].forEach(i => { document.getElementById('rv' + i).textContent = S[i].fmt(S[i].d.at(-1)); });
  }

  // ----- hero trace -----
  const traceCv = document.getElementById('traceCanvas'), trace = document.getElementById('trace'), tip = document.getElementById('traceTip');
  const T = walk(62, 44, 12, 10, 92);
  let tShift = 0, hoverX = null, lastTick = performance.now();
  const hero = document.querySelector('.hero'), stage = document.querySelector('.stage');
  const sizeTrace = () => hero.style.setProperty('--trace-h', (stage.offsetTop + 24) + 'px');
  sizeTrace(); addEventListener('resize', sizeTrace);
  function renderTrace() {
    const { ctx, w, h } = fit(traceCv);
    ctx.clearRect(0, 0, w, h);
    const copyEnd = document.querySelector('.cta-row').getBoundingClientRect().bottom - trace.getBoundingClientRect().top;
    const top = Math.max(40, copyEnd + 48), bot = 84, n = T.length, step = w / (n - 2);
    const y = v => top + (1 - v / 100) * (h - top - bot);
    ctx.strokeStyle = C.line; ctx.lineWidth = 1; ctx.fillStyle = C.muted; ctx.font = '12px ' + tok('--body');
    [[50, '50%'], [100, '100%']].forEach(([v, l]) => { const yy = Math.round(y(v)) + .5; ctx.beginPath(); ctx.moveTo(0, yy); ctx.lineTo(w, yy); ctx.stroke(); ctx.fillText(l, w - ctx.measureText(l).width - 16, yy - 8); });
    const pts = T.map((v, i) => [i * step - tShift * step, y(v)]);
    ctx.beginPath(); pts.forEach(([x, yy], i) => i ? ctx.lineTo(x, yy) : ctx.moveTo(x, yy));
    ctx.strokeStyle = C.accent; ctx.lineWidth = 2; ctx.lineJoin = 'round'; ctx.stroke();
    ctx.lineTo(pts.at(-1)[0], h - bot); ctx.lineTo(pts[0][0], h - bot); ctx.closePath(); ctx.fillStyle = C.accent + '14'; ctx.fill();
    if (hoverX != null) {
      let i = Math.max(1, Math.min(n - 1, Math.ceil(hoverX / step + tShift)));
      const [x0, y0] = pts[i - 1], [x1, y1] = pts[i], f = (hoverX - x0) / (x1 - x0 || 1);
      const yy = y0 + (y1 - y0) * f, val = T[i - 1] + (T[i] - T[i - 1]) * f;
      ctx.strokeStyle = '#3a3d46'; ctx.beginPath(); ctx.moveTo(Math.round(hoverX) + .5, top - 10); ctx.lineTo(Math.round(hoverX) + .5, h - bot); ctx.stroke();
      ctx.fillStyle = C.accent; ctx.beginPath(); ctx.arc(hoverX, yy, 5, 0, 7); ctx.fill();
      const secs = Math.round((1 - hoverX / w) * 60);
      tip.innerHTML = `${secs < 1 ? 'Now' : secs + ' s ago'}<br><i></i><b>CPU ${Math.round(val)}%</b>`;
      const tw = tip.offsetWidth; tip.style.left = Math.min(w - tw - 12, hoverX + 14) + 'px'; tip.style.top = Math.max(8, yy - 64) + 'px';
      tip.style.opacity = 1;
    } else tip.style.opacity = 0;
  }
  trace.addEventListener('pointermove', e => { const r = trace.getBoundingClientRect(); hoverX = e.clientX - r.left; if (reduce) renderTrace(); });
  trace.addEventListener('pointerleave', () => { hoverX = null; if (reduce) renderTrace(); });

  function tick() {
    S.forEach(s => { s.d.push(next(s.d.at(-1), s.base, s.spread, s.lo, s.hi, s.max < 100)); s.d.shift(); });
    coreS.forEach(s => { s.d.push(next(s.d.at(-1), s.base, s.spread, s.lo, s.hi)); s.d.shift(); });
    renderWindow(); renderCards();
  }
  renderWindow(); renderCards(); renderTrace();

  // hero trace scrolls smoothly; panels tick once a second
  let acc = 0;
  function frame(now) {
    const dt = now - lastTick; lastTick = now;
    tShift += dt / 1000; // one point per second
    if (tShift >= 1) { tShift -= 1; T.push(next(T.at(-1), 44, 12, 10, 92)); T.shift(); document.getElementById('traceNow').textContent = Math.round(T.at(-2)) + '%'; }
    acc += dt; if (acc >= 1000) { acc = 0; tick(); }
    renderTrace(); requestAnimationFrame(frame);
  }
  if (!reduce) requestAnimationFrame(frame);
  addEventListener('resize', () => { renderWindow(); renderCards(); renderTrace(); });

  // ----- window settles flat as you scroll -----
  const win = document.getElementById('win'), nav = document.getElementById('nav');
  function onScroll() {
    nav.classList.toggle('scrolled', scrollY > 8);
    if (reduce) return;
    const r = win.getBoundingClientRect(), p = Math.min(1, Math.max(0, (innerHeight - r.top) / (innerHeight * .9)));
    win.style.setProperty('--tilt', (16 * (1 - p)).toFixed(2) + 'deg');
  }
  addEventListener('scroll', onScroll, { passive: true }); onScroll();

  // ----- rail scroll-spy -----
  const railLinks = [...document.querySelectorAll('#rail a')];
  const io = new IntersectionObserver(es => es.forEach(e => {
    if (e.isIntersecting) railLinks.forEach(a => {
      const on = a.getAttribute('href') === '#' + e.target.id; a.classList.toggle('on', on);
      if (on && innerWidth <= 980) a.scrollIntoView({ block: 'nearest', inline: 'center', behavior: reduce ? 'auto' : 'smooth' });
    });
  }), { rootMargin: '-45% 0px -50% 0px' });
  document.querySelectorAll('.panel').forEach(p => io.observe(p));
  const macro = document.getElementById('macroDemo');
  new IntersectionObserver(es => es.forEach(e => macro.classList.toggle('inview', e.isIntersecting))).observe(macro);

  // ----- memory demo -----
  const memBtn = document.getElementById('memBtn'), bars = [...document.querySelectorAll('#memBar i')];
  const before = [24, 17, 13, 11, 13], after = [17, 15, 8, 10, 13];
  let cleaned = false;
  memBtn.addEventListener('click', () => {
    if (cleaned) { bars.forEach((b, i) => b.style.width = before[i] + '%'); cleaned = false;
      document.getElementById('memDot').style.background = ''; document.getElementById('memVerdict').textContent = 'Memory is getting full';
      document.getElementById('memSub').textContent = '78% in use. Games may stutter when it fills up.'; memBtn.textContent = 'Optimize memory'; memBtn.classList.add('primary');
      document.getElementById('memResult').textContent = 'Last run: never on this PC.'; return; }
    memBtn.disabled = true; memBtn.textContent = 'Optimizing…';
    bars.forEach((b, i) => b.style.width = after[i] + '%');
    const res = document.getElementById('memResult'); let gb = 0; const target = 4.1, t0 = performance.now();
    const step = now => { const k = Math.min(1, (now - t0) / 1400); gb = target * (1 - Math.pow(1 - k, 3));
      res.innerHTML = `Freed about <b class="num">${gb.toFixed(1)} GB</b> so far. Watching for 10 seconds while apps settle.`;
      if (k < 1) requestAnimationFrame(step); else {
        res.innerHTML = `Freed about <b class="num">${target} GB</b> from 7 apps. Apps load back what they need, so this drifts up over time.`;
        document.getElementById('memDot').style.background = 'var(--ok)'; document.getElementById('memVerdict').textContent = 'Memory looks fine';
        document.getElementById('memSub').textContent = '63% in use. Plenty of room.'; memBtn.disabled = false; memBtn.textContent = 'Reset demo'; memBtn.classList.remove('primary'); cleaned = true; } };
    requestAnimationFrame(step);
  });

  // ----- debloater demo -----
  const boxes = [...document.querySelectorAll('#dlist input')], apply = document.getElementById('dApply'), undo = document.getElementById('dUndo'), dmsg = document.getElementById('dmsg');
  const count = () => boxes.filter(b => b.checked && !b.disabled).length;
  function syncD() { const c = count(); apply.textContent = `Apply ${c} change${c === 1 ? '' : 's'}`; apply.disabled = !c; if (!undo.dataset.applied) dmsg.textContent = `${c} change${c === 1 ? '' : 's'} selected. Nothing has changed yet.`; }
  boxes.forEach(b => b.addEventListener('change', syncD));
  apply.addEventListener('click', () => {
    let n = 0; boxes.forEach(b => { if (b.checked && !b.disabled) { b.disabled = true; n++; const st = b.closest('.drow').querySelector('.st'); st.textContent = 'Applied'; st.classList.add('done'); } });
    undo.disabled = false; undo.dataset.applied = 1; dmsg.textContent = `Applied ${n} change${n === 1 ? '' : 's'}. Undo puts ${n === 1 ? 'it' : 'them'} back.`; syncD();
  });
  undo.addEventListener('click', () => {
    boxes.forEach(b => { if (b.disabled) { b.disabled = false; const st = b.closest('.drow').querySelector('.st'); st.textContent = 'Ready'; st.classList.remove('done'); } });
    undo.disabled = true; delete undo.dataset.applied; syncD(); dmsg.textContent = 'Undone. Your PC is as it was.';
  });

  // ----- system profiles -----
  const pboxes = [...document.querySelectorAll('#plist input')], pApply = document.getElementById('pApply'), pRestore = document.getElementById('pRestore'),
    pmsg = document.getElementById('pmsg'), plog = document.getElementById('plog'), psub = document.getElementById('profSub');
  const auto = () => pboxes.slice(0, 4).filter(b => b.checked).length;
  const syncP = () => { psub.textContent = `${pboxes.filter(b => b.checked).length} of 5 changes selected`; pApply.disabled = !pboxes.some(b => b.checked) || pApply.dataset.done; };
  pboxes.forEach(b => b.addEventListener('change', syncP));
  pApply.addEventListener('click', () => {
    const steps = ['Restore point created', 'Registry backup saved'];
    pboxes.forEach((b, i) => { if (b.checked) steps.push(i === 4 ? 'NVIDIA steps opened as a guide' : b.closest('.drow').querySelector('b').textContent); });
    pApply.disabled = true; pApply.dataset.done = 1; pboxes.forEach(b => b.disabled = true); plog.innerHTML = '';
    let t = 0; const t0 = new Date(2026, 10, 14, 21, 42, 5);
    steps.forEach((s, i) => setTimeout(() => {
      const ts = new Date(t0.getTime() + i * 1400).toTimeString().slice(0, 8);
      plog.insertAdjacentHTML('beforeend', `<li>${ts} <svg class="ic" style="width:13px;height:13px;color:var(--ok);vertical-align:-2px"><use href="#i-check"/></svg> ${s}</li>`);
      if (i === steps.length - 1) { pmsg.textContent = `Applied ${auto()} changes. Restore puts them back.`; pRestore.disabled = false;
        pboxes.forEach((b, j) => { if (b.checked) { const st = b.closest('.drow').querySelector('.st'); st.textContent = j === 4 ? 'Guided' : 'Applied'; if (j < 4) st.classList.add('done'); } }); }
    }, reduce ? 0 : (t += 380)));
  });
  pRestore.addEventListener('click', () => {
    pboxes.forEach(b => { b.disabled = false; const st = b.closest('.drow').querySelector('.st'); st.classList.remove('done'); st.textContent = b === pboxes[4] ? 'Guided' : 'Ready'; });
    const ts = new Date(2026, 10, 14, 21, 44, 30).toTimeString().slice(0, 8);
    plog.insertAdjacentHTML('beforeend', `<li>${ts} <svg class="ic" style="width:13px;height:13px;color:var(--ok);vertical-align:-2px"><use href="#i-check"/></svg> Restored the settings Toolboks changed</li>`);
    pRestore.disabled = true; delete pApply.dataset.done; pmsg.textContent = 'Restored. Windows is set as it was before.'; syncP();
  });
  pboxes[4].closest('.drow').querySelector('.st').textContent = 'Guided';

  // ----- event watch -----
  const evs = [
    ['ok', 'Info', 'Windows Update installed 2 updates'], ['watch', 'Warning', 'A driver took 4 s to start'], ['ok', 'Info', 'Your PC woke from sleep'],
    ['bad', 'Error', 'Discord stopped responding and was closed'], ['ok', 'Info', 'A USB drive was connected'], ['watch', 'Warning', 'Disk C is nearly full'],
    ['ok', 'Info', 'Time was synced with the internet'], ['ok', 'Info', 'Defender finished a quick scan, nothing found'],
  ];
  const tb = document.querySelector('#etable tbody'); let ei = 0, clock = new Date(2026, 10, 14, 21, 40, 12);
  function addEv(anim) {
    const [c, l, t] = evs[ei++ % evs.length]; clock = new Date(clock.getTime() + 37000 + Math.random() * 90000);
    const tr = document.createElement('tr'); if (anim) tr.className = 'new';
    tr.innerHTML = `<td>${clock.toTimeString().slice(0, 8)}</td><td><span class="lv" style="--c:var(--${c})">${l}</span></td><td>${t}</td>`;
    tb.prepend(tr); while (tb.children.length > 6) tb.lastChild.remove();
  }
  for (let i = 0; i < 6; i++) addEv(false);
  if (!reduce) setInterval(() => addEv(true), 2600);

  // ----- audio -----
  const meter = document.getElementById('meter'); const mb = Array.from({ length: 28 }, () => meter.appendChild(document.createElement('i')));
  let ph = 0; const gain = document.getElementById('gain');
  function meterTick() { ph += .35; const g = +gain.value / 24;
    mb.forEach((b, i) => { const v = Math.max(.06, Math.min(1, (.25 + .45 * g) * (0.55 + 0.45 * Math.sin(ph + i * .5)) + Math.random() * .22)); b.style.height = (v * 100).toFixed(0) + '%'; b.classList.toggle('hot', v > .86); }); }
  meterTick(); if (!reduce) setInterval(meterTick, 120);
  gain.addEventListener('input', () => { document.getElementById('gainOut').textContent = '+' + gain.value + ' dB'; if (reduce) meterTick(); });
  const listen = document.getElementById('listen'); listen.addEventListener('input', () => document.getElementById('listenOut').textContent = listen.value + '%');

  // ----- add-on browser -----
  const vb = '<span class="vbadge"><svg class="ic" aria-hidden="true"><use href="#i-verified"/></svg>Verified</span>';
  const ACCESS = { host_metrics: 'Read memory use and uptime', local_preferences: 'Keep its own settings on this PC', filesystem_read: 'Read file and folder sizes on drives you choose',
    user_exports: 'Save reports when you ask', system_changes: 'Change Windows settings, with a preview and undo each time', input_capture: 'Record keyboard and mouse while you record', system_tuning: 'Change Windows performance settings after a restore point and registry backup', audio_devices: 'Use your microphone and audio outputs' };
  const ADDONS = [
    { id: 'mem', name: 'Memory Cleaner', icon: 'i-chip', cat: 'System', v: true, pub: 'Toolboks', d: 'Free up memory without closing anything.', caps: ['host_metrics', 'local_preferences'], inst: true },
    { id: 'mon', name: 'Resource Monitor', icon: 'i-pulse', cat: 'System', v: true, pub: 'Toolboks', d: 'Live graphs for CPU, GPU, memory, disk and network.', caps: ['host_metrics'], inst: true },
    { id: 'deb', name: 'Debloater', icon: 'i-sliders', cat: 'System', v: true, pub: 'Toolboks', d: 'Remove clutter from Windows, with undo for every change.', caps: ['system_changes', 'local_preferences'], inst: false },
    { id: 'prof', name: 'System Profiles', icon: 'i-gauge', cat: 'System', v: true, pub: 'Toolboks', d: 'Set Windows up for gaming, with a restore point first.', caps: ['system_tuning'], inst: false },
    { id: 'evt', name: 'Event Watch', icon: 'i-shield', cat: 'System', v: true, pub: 'Toolboks', d: 'Windows events, explained in one line each.', caps: ['host_metrics'], inst: false },
    { id: 'disk', name: 'Disk Manager', by: 'by stratum', icon: 'i-drive', cat: 'Storage', v: true, pub: 'Toolboks', d: 'See what fills your drives and optimize them.', caps: ['filesystem_read', 'user_exports'], inst: false },
    { id: 'mac', name: 'Macro Manager', icon: 'i-cursor', cat: 'Automation', v: true, pub: 'Toolboks', d: 'Record, edit and replay keyboard and mouse actions.', caps: ['input_capture', 'local_preferences'], inst: false },
    { id: 'aud', name: 'Audio Gainer', icon: 'i-mic', cat: 'Audio', v: true, pub: 'Toolboks', d: 'Boost a quiet microphone and route it to other apps.', caps: ['audio_devices', 'local_preferences'], inst: false },
    { id: 'hello', name: 'Hello System', icon: 'i-grid', cat: 'Developer tools', v: false, pub: 'Example developer', d: 'The SDK starter: live memory readings and saved preferences.', caps: ['host_metrics', 'local_preferences'], inst: false },
    { id: 'bat', name: 'Battery Report', icon: 'i-pulse', cat: 'System', v: false, pub: 'Example publisher', d: 'Battery wear and charge history in plain words.', caps: ['host_metrics', 'user_exports'], inst: false },
    { id: 'dup', name: 'Duplicate Finder', icon: 'i-file', cat: 'Storage', v: false, pub: 'Example publisher', d: 'Find identical files and choose which copy to keep.', caps: ['filesystem_read'], inst: false },
    { id: 'clip', name: 'Clipboard History', icon: 'i-file', cat: 'Automation', v: false, pub: 'Example publisher', d: 'The last 50 things you copied, searchable.', caps: ['local_preferences'], inst: false },
    { id: 'boot', name: 'Startup Timer', icon: 'i-pulse', cat: 'System', v: false, pub: 'Example publisher', d: 'How long each app adds to your boot time.', caps: ['host_metrics'], inst: false },
    { id: 'eq', name: 'Mic Noise Gate', icon: 'i-mic', cat: 'Audio', v: false, pub: 'Example publisher', d: 'Mute your microphone between words.', caps: ['audio_devices', 'local_preferences'], inst: false },
    { id: 'big', name: 'Large File Sweep', icon: 'i-drive', cat: 'Storage', v: false, pub: 'Example publisher', d: 'The biggest files you haven\'t opened in a year.', caps: ['filesystem_read'], inst: false },
    { id: 'win', name: 'Window Snapper', icon: 'i-grid', cat: 'Automation', v: false, pub: 'Example publisher', d: 'Save window layouts and restore them with one key.', caps: ['input_capture', 'local_preferences'], inst: false },
  ];
  // the drifting wall of tool ideas: shows scale, not real listings
  const IDEAS = [
    ['Fan curves', 'System', 'i-sliders'], ['Wi-Fi signal map', 'Network', 'i-pulse'], ['Battery report', 'System', 'i-pulse'], ['Duplicate finder', 'Storage', 'i-file'],
    ['Clipboard history', 'Automation', 'i-file'], ['Startup timer', 'System', 'i-pulse'], ['Mic noise gate', 'Audio', 'i-mic'], ['Driver checker', 'System', 'i-shield'],
    ['Game mode switch', 'Gaming', 'i-sliders'], ['Port viewer', 'Network', 'i-grid'], ['Hotkey launcher', 'Automation', 'i-cursor'], ['Temp file sweep', 'Storage', 'i-drive'],
    ['Screen color picker', 'Developer tools', 'i-grid'], ['Power plan swap', 'System', 'i-sliders'], ['Audio device switcher', 'Audio', 'i-mic'], ['Folder size watch', 'Storage', 'i-drive'],
    ['Ping graph', 'Network', 'i-pulse'], ['Window snapper', 'Automation', 'i-grid'], ['Service manager', 'System', 'i-shield'], ['JSON viewer', 'Developer tools', 'i-file'],
    ['FPS overlay', 'Gaming', 'i-pulse'], ['Bluetooth battery', 'System', 'i-chip'], ['Download sorter', 'Automation', 'i-file'], ['SMART health', 'Storage', 'i-drive'],
  ];
  const VER = [['Memory Cleaner', 'i-chip'], ['Resource Monitor', 'i-pulse'], ['Debloater', 'i-sliders'], ['Event Watch', 'i-shield'], ['System Profiles', 'i-gauge'], ['Disk Manager', 'i-drive'], ['Macro Manager', 'i-cursor'], ['Audio Gainer', 'i-mic']];
  document.querySelectorAll('.wtrack').forEach((t, r) => {
    const chips = [];
    IDEAS.slice(r * 8, r * 8 + 8).forEach((x, i) => {
      chips.push(`<span class="chip"><span class="aic"><svg class="ic"><use href="#${x[2]}"/></svg></span>${x[0]}<small>${x[1]}</small></span>`);
      if (i % 3 === 1) { const v = VER[(r * 3 + i) % VER.length]; chips.push(`<span class="chip v"><span class="aic"><svg class="ic"><use href="#${v[1]}"/></svg></span>${v[0]}${vb}</span>`); }
    });
    const html = chips.join(''); t.innerHTML = html + html; t.style.setProperty('--dur', (80 + r * 18) + 's');
  });
  const alist = document.getElementById('alist'), adetail = document.getElementById('adetail'), q = document.getElementById('storeQ');
  let cat = 'all', sel = 'mac';
  function renderList() {
    const term = q.value.trim().toLowerCase();
    const rows = ADDONS.filter(a => (cat === 'all' || a.cat === cat) && (!term || (a.name + ' ' + a.d + ' ' + a.pub).toLowerCase().includes(term)));
    alist.innerHTML = rows.length ? rows.map(a => `<li class="arow${a.id === sel ? ' on' : ''}" data-id="${a.id}" tabindex="0">
      <span class="aic"><svg class="ic"><use href="#${a.icon}"/></svg></span>
      <span style="min-width:0"><b>${a.name}</b>${a.v ? vb : `<span class="pub">${a.pub}</span>`}<p>${a.d}</p></span>
      <span class="astate${a.inst ? ' ok' : ''}">${a.inst ? '<svg class="ic"><use href="#i-check"/></svg>Installed' : a.cat}</span></li>`).join('')
      : `<li class="aempty">No add-ons match "${q.value.replace(/[<>&"]/g, '')}". Try another word or pick All.</li>`;
    if (rows.length) alist.insertAdjacentHTML('beforeend', `<li class="lfoot">${rows.length} example listings. The real browser grows as people publish tools.</li>`);
    if (rows.length && !rows.some(a => a.id === sel)) { sel = rows[0].id; renderList(); return; }
    renderDetail();
  }
  function renderDetail() {
    const a = ADDONS.find(x => x.id === sel); if (!a) { adetail.innerHTML = ''; return; }
    adetail.innerHTML = `<div><h3>${a.name}</h3><div class="meta">${a.by ? a.by + ' · ' : ''}${a.pub} · ${a.cat}</div></div>
      <p>${a.d}</p>
      ${a.v ? `<div class="trust"><svg class="ic"><use href="#i-verified"/></svg><span>Toolboks verified. Built and checked by the Toolboks team.</span></div>`
            : `<div class="trust plain"><svg class="ic"><use href="#i-file"/></svg><span>Not verified. The publisher name is what the package says; review the access below before you install.</span></div>`}
      <div class="access"><h4>It can</h4>${a.caps.map(c => `<div><svg class="ic"><use href="#i-check"/></svg><span>${ACCESS[c]}</span></div>`).join('')}</div>
      <button class="btn ${a.inst ? '' : 'primary'}" type="button" id="aBtn">${a.inst ? 'Turn off' : 'Install'}</button>`;
    document.getElementById('aBtn').addEventListener('click', e => {
      const b = e.currentTarget;
      if (a.inst) { a.inst = false; renderList(); return; }
      b.disabled = true; b.textContent = 'Installing…';
      setTimeout(() => { a.inst = true; renderList(); }, reduce ? 0 : 900);
    });
  }
  alist.addEventListener('click', e => { const r = e.target.closest('.arow'); if (r) { sel = r.dataset.id; renderList(); } });
  alist.addEventListener('keydown', e => { const r = e.target.closest('.arow'); if (r && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); sel = r.dataset.id; renderList(); alist.querySelector(`[data-id="${sel}"]`)?.focus(); } });
  document.getElementById('cats').addEventListener('click', e => { const b = e.target.closest('button'); if (!b) return;
    cat = b.dataset.cat; document.querySelectorAll('#cats button').forEach(x => x.classList.toggle('on', x === b)); renderList(); });
  q.addEventListener('input', renderList);
  renderList();

  document.getElementById('bannerX').addEventListener('click', () => { document.getElementById('banner').style.opacity = .4; });
})();

// The version shown on the page is the newest GitHub release. The text in the HTML is the fallback
// when the lookup fails (offline, blocked, rate limited), so the page never shows an empty version.
(() => {
  try {
    fetch('https://api.github.com/repos/Gusseti/toolboks/releases/latest', {
      headers: { Accept: 'application/vnd.github+json' },
    })
      .then((r) => (r.ok ? r.json() : null))
      .then((release) => {
        const version = release && String(release.tag_name || '').replace(/^v/, '');
        if (!version || !/^\d+\.\d+\.\d+$/.test(version)) return;
        document.querySelectorAll('[data-version]').forEach((el) => { el.textContent = version; });
      })
      .catch(() => {});
  } catch (e) { /* the fallback text stays */ }
})();
