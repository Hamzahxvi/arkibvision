(function () {
  'use strict';

  var BTN_ID = 'rf-admin-rail-btn';
  var STAGE_ID = 'rf-admin-stage';
  var API_BASE = 'http://' + (window.location.hostname || 'localhost') + ':8005';

  var TRASH_SVG = '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/><line x1="10" x2="10" y1="11" y2="17"/><line x1="14" x2="14" y1="11" y2="17"/></svg>';
  var TRASH_LG_SVG = '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/><line x1="10" x2="10" y1="11" y2="17"/><line x1="14" x2="14" y1="11" y2="17"/></svg>';
  var DOTS_SVG = '<svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="5" r="1.2" fill="currentColor"/><circle cx="12" cy="12" r="1.2" fill="currentColor"/><circle cx="12" cy="19" r="1.2" fill="currentColor"/></svg>';
  var EDIT_SVG = '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"/></svg>';

  var state = {
    open: false,
    authed: false,
    user: '',
    tab: 'approvals',
    filter: 'all',
    search: '',
    requests: [],
    profiles: [],
    peopleGroups: [],
    selectedGroup: 'all',
    peopleGroupSearch: '',
    locations: [],
    locationGroups: [],
    selectedLocGroup: 'all',
    locGroupSearch: '',
    documents: [],
    documentGroups: [],
    selectedDocGroup: 'all',
    docGroupSearch: '',
    health: null,
    integrity: null,
    labHtml: '',
    testResultHtml: '',
    testPreview: '',
    testFile: null,
    testFileName: '',
    testThresholdPct: '88',
    testUnrecognized: false,
    testEnrollMode: 'existing',
    testEnrollFilter: '',
    testEnrollSlug: '',
    testEnrollNewName: '',
    testEnrollNewGroup: '',
    railEl: null,
    pollTimer: null,
    editingProfile: null,
    selectedPhoto: null,
    editingLoc: null,
    editingDoc: null
  };

  function restoreFocus(id, pos) {
    var el = document.getElementById(id);
    if (!el) return;
    el.focus();
    try {
      if (typeof pos === 'number' && typeof el.setSelectionRange === 'function') {
        el.setSelectionRange(pos, pos);
      }
    } catch (e) {}
  }

  function esc(v) {
    return String(v == null ? '' : v).replace(/[&<>"']/g, function (c) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c];
    });
  }

  function apiUrl(path) {
    if (!path) return '';
    if (path.indexOf('http://') === 0 || path.indexOf('https://') === 0) return path;
    return API_BASE + path;
  }

  async function api(path, opts) {
    var cfg = Object.assign({ credentials: 'include' }, opts || {});
    var r = await fetch(apiUrl(path), cfg);
    var data = {};
    try { data = await r.json(); } catch (e) {}
    if (r.status === 401 && path !== '/admin/api/login' && path !== '/admin/api/session') {
      state.authed = false;
      render();
      throw new Error('Please sign in to the Admin Portal.');
    }
    return { ok: r.ok, status: r.status, data: data };
  }

  function toast(msg, ok) {
    var existing = document.querySelector('.rf-toast');
    if (existing) existing.remove();
    var el = document.createElement('div');
    el.className = 'rf-toast ' + (ok === false ? 'err' : 'ok');
    el.textContent = msg;
    document.body.appendChild(el);
    setTimeout(function () {
      el.style.opacity = '0';
      setTimeout(function () { el.remove(); }, 260);
    }, 3000);
  }

  function openViewer(src) {
    var v = document.createElement('div');
    v.className = 'rf-viewer';
    v.innerHTML = '<img src="' + esc(src) + '" alt="Preview"/>';
    v.addEventListener('click', function () { v.remove(); });
    document.body.appendChild(v);
  }

  function closeMenus() {
    document.querySelectorAll('.rf-menu-pop').forEach(function (m) { m.remove(); });
  }

  function openFolderMenu(anchorBtn, opts) {
    closeMenus();
    var rect = anchorBtn.getBoundingClientRect();
    var pop = document.createElement('div');
    pop.className = 'rf-menu-pop';
    pop.setAttribute('role', 'menu');
    pop.style.top = Math.min(window.innerHeight - 110, Math.round(rect.bottom + 6)) + 'px';
    pop.style.left = Math.min(window.innerWidth - 190, Math.max(8, Math.round(rect.left - 120))) + 'px';
    pop.innerHTML =
      '<button type="button" class="rf-menu-item" data-act="rename">' + EDIT_SVG + '<span>Rename folder</span></button>' +
      '<button type="button" class="rf-menu-item danger" data-act="delete">' + TRASH_SVG + '<span>Delete folder</span></button>';
    pop.querySelector('[data-act="rename"]').onclick = function (e) {
      e.stopPropagation();
      closeMenus();
      if (opts.onRename) opts.onRename();
    };
    pop.querySelector('[data-act="delete"]').onclick = function (e) {
      e.stopPropagation();
      closeMenus();
      if (opts.onDelete) opts.onDelete();
    };
    document.body.appendChild(pop);
  }

  function openConfirmModal(opts) {
    closeMenus();
    var wrap = document.createElement('div');
    wrap.className = 'rf-modal-backdrop';
    wrap.style.zIndex = '90';
    wrap.innerHTML =
      '<div class="rf-modal" style="width:min(460px,100%)">' +
      '<div class="rf-confirm-icon">' + TRASH_LG_SVG + '</div>' +
      '<h2 style="margin-bottom:8px">' + esc(opts.title || 'Confirm Delete') + '</h2>' +
      '<p style="font-size:.86rem;color:rgb(var(--text-secondary));line-height:1.55;margin:0 0 18px">' + esc(opts.message || 'Are you sure you want to continue?') + '</p>' +
      '<div class="rf-actions" style="justify-content:flex-end;margin-top:0">' +
      '<button type="button" class="rf-btn" id="rf-cfm-cancel">Cancel</button>' +
      '<button type="button" class="rf-btn ' + (opts.primary ? 'rf-btn-primary' : 'rf-btn-danger-solid') + '" id="rf-cfm-ok">' + (opts.primary ? '' : TRASH_SVG) + '<span>' + esc(opts.confirmLabel || 'Delete') + '</span></button>' +
      '</div></div>';
    document.body.appendChild(wrap);
    var cancel = function () {
      wrap.remove();
      if (opts.onCancel) opts.onCancel();
    };
    wrap.onclick = function (e) { if (e.target === wrap) cancel(); };
    wrap.querySelector('#rf-cfm-cancel').onclick = cancel;
    wrap.querySelector('#rf-cfm-ok').onclick = async function () {
      var okBtn = wrap.querySelector('#rf-cfm-ok');
      okBtn.disabled = true;
      try {
        if (opts.onConfirm) await opts.onConfirm();
      } finally {
        wrap.remove();
      }
    };
  }

  function computeRailRight() {
    if (state.railEl && document.body.contains(state.railEl)) {
      var rect = state.railEl.getBoundingClientRect();
      if (rect.width >= 36 && rect.width <= 96) return Math.round(rect.right);
    }
    return 52;
  }

  function positionStage() {
    var stage = document.getElementById(STAGE_ID);
    if (stage) stage.style.left = computeRailRight() + 'px';
  }

  function ensureStage() {
    var stage = document.getElementById(STAGE_ID);
    if (!stage) {
      stage = document.createElement('div');
      stage.id = STAGE_ID;
      stage.setAttribute('role', 'region');
      stage.setAttribute('aria-label', 'Admin Portal');
      stage.style.display = 'none';
      document.body.appendChild(stage);
    }
    return stage;
  }

  function setButtonActive(active) {
    var btn = document.getElementById(BTN_ID);
    if (!btn) return;
    btn.classList.toggle('rf-active', active);
    btn.setAttribute('aria-pressed', active ? 'true' : 'false');
  }

  async function refreshAll(manual) {
    if (!state.authed) return;
    try {
      var res = await Promise.all([
        api('/admin/api/requests?ts=' + Date.now()),
        api('/admin/api/profiles?ts=' + Date.now()),
        api('/admin/api/people-groups?ts=' + Date.now()),
        api('/admin/api/locations?ts=' + Date.now()),
        api('/admin/api/location-groups?ts=' + Date.now()),
        api('/admin/api/documents?ts=' + Date.now()),
        api('/admin/api/document-groups?ts=' + Date.now()),
        api('/admin/api/system/health?ts=' + Date.now())
      ]);
      if (res[0].ok && Array.isArray(res[0].data)) state.requests = res[0].data;
      if (res[1].ok && Array.isArray(res[1].data)) state.profiles = res[1].data;
      if (res[2].ok && Array.isArray(res[2].data)) state.peopleGroups = res[2].data;
      if (res[3].ok && Array.isArray(res[3].data)) state.locations = res[3].data;
      if (res[4].ok && Array.isArray(res[4].data)) state.locationGroups = res[4].data;
      if (res[5].ok && Array.isArray(res[5].data)) state.documents = res[5].data;
      if (res[6].ok && Array.isArray(res[6].data)) state.documentGroups = res[6].data;
      if (res[7].ok) state.health = res[7].data;
      render();
      if (manual) toast('Admin data refreshed');
    } catch (e) {}
  }

  async function checkSessionAndLoad() {
    try {
      var r = await api('/admin/api/session?ts=' + Date.now());
      state.authed = !!(r.ok && r.data && r.data.authenticated);
      state.user = (r.data && r.data.user) || '';
    } catch (e) {
      state.authed = false;
    }
    render();
    if (state.authed) await refreshAll(false);
  }

  function openAdmin() {
    var closeDrawerBtn = document.querySelector('[data-testid="close-sidebar-button"]');
    if (closeDrawerBtn) {
      try { closeDrawerBtn.click(); } catch (e) {}
    }
    var stage = ensureStage();
    state.open = true;
    positionStage();
    stage.style.display = 'block';
    setButtonActive(true);
    setTimeout(positionStage, 120);
    checkSessionAndLoad();
    if (!state.pollTimer) {
      state.pollTimer = setInterval(function () {
        if (state.open && state.authed && !document.querySelector('.rf-modal-backdrop')) {
          api('/admin/api/requests?ts=' + Date.now()).then(function (r) {
            if (r.ok && Array.isArray(r.data)) {
              state.requests = r.data;
              if (state.tab === 'approvals') renderBody();
              updatePendingBadge();
            }
          }).catch(function () {});
        }
      }, 8000);
    }
  }

  function closeAdmin() {
    if (!state.open) return;
    state.open = false;
    var stage = document.getElementById(STAGE_ID);
    if (stage) stage.style.display = 'none';
    setButtonActive(false);
  }

  function updatePendingBadge() {
    var b = document.getElementById('rf-pending-badge');
    if (b) {
      var cnt = state.requests.filter(function (x) { return x.status === 'pending'; }).length;
      b.textContent = String(cnt);
    }
  }

  function render() {
    var stage = ensureStage();
    if (!state.authed) {
      stage.innerHTML =
        '<div class="rf-login-wrap"><div class="rf-login-card">' +
        '<span class="rf-pill">🛡️ ARKIBVISION · ADMIN PORTAL</span>' +
        '<h2>Sign in to Admin Portal</h2>' +
        '<p>Manage approvals, enrolled people, archival locations, documents, file integrity, and system health right inside LibreChat.</p>' +
        '<div id="rf-login-err" class="rf-err-banner" style="display:none"></div>' +
        '<form id="rf-login-form">' +
        '<div class="rf-field" style="margin-bottom:14px"><label for="rf-u">Username</label><input class="rf-input" id="rf-u" autocomplete="username" required/></div>' +
        '<div class="rf-field" style="margin-bottom:18px"><label for="rf-p">Password</label><input class="rf-input" id="rf-p" type="password" autocomplete="current-password" required/></div>' +
        '<button type="submit" class="rf-btn rf-btn-primary" style="width:100%;padding:10px">Sign in</button>' +
        '</form></div></div>';
      var form = document.getElementById('rf-login-form');
      form.addEventListener('submit', async function (ev) {
        ev.preventDefault();
        var err = document.getElementById('rf-login-err');
        err.style.display = 'none';
        try {
          var r = await api('/admin/api/login', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
              username: document.getElementById('rf-u').value,
              password: document.getElementById('rf-p').value
            })
          });
          if (!r.ok) {
            err.textContent = (r.data && r.data.reason) || 'Invalid username or password.';
            err.style.display = 'block';
            return;
          }
          state.authed = true;
          state.user = (r.data && r.data.user) || 'admin';
          render();
          await refreshAll(false);
          toast('Signed in to Admin Portal');
        } catch (e) {
          err.textContent = 'Could not reach local ArkibVision service (port 8005).';
          err.style.display = 'block';
        }
      });
      return;
    }

    var pendingCnt = state.requests.filter(function (x) { return x.status === 'pending'; }).length;
    var tabs = [
      { id: 'approvals', label: '📋 Approvals', count: pendingCnt },
      { id: 'people', label: '👥 People', count: state.profiles.length },
      { id: 'locations', label: '📍 Locations', count: state.locations.length },
      { id: 'documents', label: '📁 Documents', count: state.documents.length },
      { id: 'system', label: '📊 System & Lab' },
      { id: 'test', label: '🔍 Test Face' }
    ];

    stage.innerHTML =
      '<div class="rf-shell">' +
      '<header class="rf-header">' +
      '<div class="rf-brand"><span>🛡️ Admin Portal</span><span class="rf-pill">LOCAL AI</span></div>' +
      '<nav class="rf-tabs">' +
      tabs.map(function (t) {
        var cntHtml = typeof t.count === 'number'
          ? '<span class="rf-tab-count"' + (t.id === 'approvals' ? ' id="rf-pending-badge"' : '') + '>' + t.count + '</span>'
          : '';
        return '<button type="button" class="rf-tab' + (state.tab === t.id ? ' active' : '') + '" data-rf-tab="' + t.id + '">' + t.label + cntHtml + '</button>';
      }).join('') +
      '</nav>' +
      '<div class="rf-header-actions">' +
      '<button type="button" class="rf-btn rf-btn-sm" id="rf-refresh-btn">↻ Refresh</button>' +
      '<button type="button" class="rf-btn rf-btn-sm" id="rf-logout-btn">↪ Log out</button>' +
      '</div></header>' +
      '<main class="rf-main" id="rf-main-body"></main>' +
      '</div>';

    stage.querySelectorAll('[data-rf-tab]').forEach(function (b) {
      b.addEventListener('click', function () {
        state.tab = b.getAttribute('data-rf-tab');
        state.search = '';
        render();
      });
    });
    document.getElementById('rf-refresh-btn').addEventListener('click', function () { refreshAll(true); });
    document.getElementById('rf-logout-btn').addEventListener('click', async function () {
      await api('/admin/api/logout', { method: 'POST' });
      state.authed = false;
      render();
      toast('Signed out');
    });

    renderBody();
  }

  function renderBody() {
    var body = document.getElementById('rf-main-body');
    if (!body) return;
    if (state.tab === 'approvals') renderApprovals(body);
    else if (state.tab === 'people') renderPeople(body);
    else if (state.tab === 'locations') renderLocations(body);
    else if (state.tab === 'documents') renderDocuments(body);
    else if (state.tab === 'system') renderSystem(body);
    else if (state.tab === 'test') renderTestFace(body);
  }

  // ─── Tab 1: Approvals ─────────────────────────────────────────────────────
  function renderApprovals(body) {
    var pending = state.requests.filter(function (x) { return x.status === 'pending'; });
    var isEnrollType = function (t) { return t === 'enrollment' || t === 'face_enrollment'; };
    var enrollCnt = pending.filter(function (x) { return isEnrollType(x.request_type); }).length;
    var updateCnt = pending.filter(function (x) { return x.request_type === 'metadata_update'; }).length;
    var ocrCnt = pending.filter(function (x) { return x.request_type === 'ocr_correction'; }).length;
    var q = state.search.toLowerCase();
    var visible = pending.filter(function (x) {
      var mf = state.filter === 'all' ||
        (state.filter === 'enrollment' ? isEnrollType(x.request_type) : x.request_type === state.filter);
      var ms = !q || (String(x.name || '') + ' ' + String(x.id || '') + ' ' + String(x.document_slug || '')).toLowerCase().indexOf(q) !== -1;
      return mf && ms;
    });

    body.innerHTML =
      '<div class="rf-section-head"><div><h2>Requests Awaiting Approval</h2><p>Review guest enrollment, profile update, and Jawi OCR correction requests before committing them to the trusted archive.</p></div></div>' +
      '<div class="rf-stats">' +
      '<div class="rf-stat"><strong>' + pending.length + '</strong><span>Waiting for review</span></div>' +
      '<div class="rf-stat"><strong>' + enrollCnt + '</strong><span>New enrollments</span></div>' +
      '<div class="rf-stat"><strong>' + updateCnt + '</strong><span>Profile updates</span></div>' +
      '<div class="rf-stat"><strong>' + ocrCnt + '</strong><span>OCR corrections</span></div>' +
      '</div>' +
      '<div class="rf-toolbar">' +
      '<input class="rf-input" id="rf-req-search" style="flex:1;min-width:220px" type="search" placeholder="Search by name, document, or request ID…" value="' + esc(state.search) + '"/>' +
      ['all', 'enrollment', 'metadata_update', 'ocr_correction'].map(function (f) {
        var lbl = f === 'all' ? 'All' : f === 'enrollment' ? 'Enrollments' : f === 'metadata_update' ? 'Updates' : 'OCR Edits';
        return '<button type="button" class="rf-btn' + (state.filter === f ? ' rf-btn-primary' : '') + '" data-rf-flt="' + f + '">' + lbl + '</button>';
      }).join('') +
      '</div>' +
      '<div class="rf-grid">' +
      (visible.length ? visible.map(function (req) {
        if (req.request_type === 'ocr_correction') {
          var pairs = Array.isArray(req.lexicon_suggestions) ? req.lexicon_suggestions : [];
          var lexHtml = pairs.length
            ? '<div style="margin-top:6px;padding:6px 8px;border-radius:6px;background:rgba(37,99,235,.08);font-size:.74rem">' +
              '<b>DBP Lexicon Pairs (' + pairs.length + '):</b> ' +
              pairs.map(function (p) { return esc(p.jawi || '') + ' → ' + esc(p.rumi || ''); }).join(', ') +
              '<label style="display:flex;align-items:center;gap:6px;margin-top:5px;cursor:pointer">' +
              '<input type="checkbox" checked data-rf-lex-chk="' + esc(req.id) + '"/> Add pairs to DBP lexicon (dbp_jawi_rumi.tsv)</label></div>'
            : '';
          return '<article class="rf-card rf-card-col">' +
            '<div style="display:flex;justify-content:space-between;align-items:center"><b>📜 ' + esc(req.document_title || req.name || 'Jawi OCR Correction') + '</b><span class="rf-meta">' + esc(req.id) + '</span></div>' +
            (req.edited_jawi_text ? '<div dir="rtl" style="padding:7px 9px;border-radius:6px;border:1px solid rgb(var(--border-light));font-family:Amiri,Traditional Arabic,serif;font-size:1.05rem;max-height:90px;overflow:auto">' + esc(req.edited_jawi_text) + '</div>' : '') +
            (req.edited_rumi_text ? '<div style="padding:7px 9px;border-radius:6px;background:rgba(16,185,129,.08);border:1px solid rgba(16,185,129,.28);font-size:.8rem;max-height:90px;overflow:auto"><b>Rumi:</b> ' + esc(req.edited_rumi_text) + '</div>' : '') +
            (req.note ? '<div style="font-size:.76rem;color:rgb(var(--text-secondary))"><b>Note:</b> ' + esc(req.note) + '</div>' : '') +
            lexHtml +
            '<div class="rf-actions">' +
            '<button type="button" class="rf-btn rf-btn-ok" style="flex:1" data-rf-act="approve" data-rf-id="' + esc(req.id) + '">✅ Approve</button>' +
            '<button type="button" class="rf-btn rf-btn-danger" style="flex:1" data-rf-act="reject" data-rf-id="' + esc(req.id) + '">' + TRASH_SVG + '<span>Reject</span></button>' +
            '</div></article>';
        }
        var meta = req.metadata || {};
        var kind = req.request_type === 'metadata_update' ? 'Profile update' : 'New enrollment';
        var srcImg = apiUrl('/admin/images/source/' + encodeURIComponent(req.id));
        var cropImg = apiUrl('/admin/images/crop/' + encodeURIComponent(req.id));
        var rows = [
          ['Action', kind],
          ['Submitted', new Date(req.submitted_at || Date.now()).toLocaleString()],
          ['Group', meta.group],
          ['Description', meta.description],
          ['Location', meta.location],
          ['Note', req.note]
        ].filter(function (p) { return p[1]; }).map(function (p) {
          return '<div><b>' + esc(p[0]) + ':</b> ' + esc(p[1]) + '</div>';
        }).join('');
        return '<article class="rf-card rf-card-col">' +
          '<div style="display:flex;justify-content:space-between;align-items:center"><b>' + esc(req.name || 'Unknown') + '</b><span class="rf-meta">' + esc(req.id) + '</span></div>' +
          '<div style="display:grid;grid-template-columns:1fr 1fr;gap:8px">' +
          '<img src="' + esc(srcImg) + '" alt="Source" style="width:100%;height:130px;object-fit:cover;border-radius:8px;border:1px solid rgb(var(--border-light))" onerror="this.style.opacity=\'.25\'"/>' +
          '<img src="' + esc(cropImg) + '" alt="Crop" style="width:100%;height:130px;object-fit:cover;border-radius:8px;border:1px solid rgb(var(--border-light))" onerror="this.style.opacity=\'.25\'"/>' +
          '</div>' +
          '<div style="font-size:.8rem;color:rgb(var(--text-secondary));line-height:1.55">' + rows + '</div>' +
          '<div class="rf-actions">' +
          '<button type="button" class="rf-btn rf-btn-ok" style="flex:1" data-rf-act="approve" data-rf-id="' + esc(req.id) + '">✅ Approve</button>' +
          '<button type="button" class="rf-btn rf-btn-danger" style="flex:1" data-rf-act="reject" data-rf-id="' + esc(req.id) + '">' + TRASH_SVG + '<span>Reject</span></button>' +
          '</div></article>';
      }).join('') : '<div class="rf-empty"><b>All clear!</b><div style="margin-top:4px;font-size:.82rem">No pending requests need your attention.</div></div>') +
      '</div>';

    document.getElementById('rf-req-search').addEventListener('input', function (e) {
      state.search = e.target.value;
      renderApprovals(body);
    });
    body.querySelectorAll('[data-rf-flt]').forEach(function (b) {
      b.addEventListener('click', function () {
        state.filter = b.getAttribute('data-rf-flt');
        renderApprovals(body);
      });
    });
    body.querySelectorAll('[data-rf-act]').forEach(function (b) {
      b.addEventListener('click', function () {
        var act = b.getAttribute('data-rf-act');
        var id = b.getAttribute('data-rf-id');
        var req = state.requests.find(function (x) { return x.id === id; });
        var lexChk = body.querySelector('[data-rf-lex-chk="' + id + '"]');
        var addLex = lexChk ? !!lexChk.checked : true;
        var runAct = async function () {
          b.disabled = true;
          try {
            var opts = { method: 'POST' };
            if (act === 'approve') {
              opts.headers = { 'Content-Type': 'application/json' };
              opts.body = JSON.stringify({ add_to_lexicon: addLex });
            }
            var r = await api('/admin/api/' + act + '/' + encodeURIComponent(id), opts);
            if (r.ok) {
              toast(act === 'approve' ? '✅ Approved: ' + (r.data.identity || id) : 'Rejected request: ' + id, act === 'approve');
              await refreshAll(false);
            } else {
              toast('Error: ' + (r.data.reason || 'failed'), false);
              b.disabled = false;
            }
          } catch (err) { toast('Error: ' + err.message, false); b.disabled = false; }
        };
        if (act === 'reject') {
          openConfirmModal({
            title: 'Reject Request',
            message: 'Reject pending request ' + id + (req && req.name ? ' for ' + req.name : '') + '? The staged request will be marked rejected.',
            confirmLabel: 'Reject Request',
            onConfirm: runAct
          });
        } else {
          runAct();
        }
      });
    });
  }

  // ─── Tab 2: Enrolled People (with Maintainable Group Folders) ─────────────
  function personGroupSlug(p) {
    return String(p.group_slug || '').trim().toLowerCase();
  }

  function renderPeople(body) {
    closeMenus();
    var q = state.search.toLowerCase();
    var validSlugs = {};
    state.peopleGroups.forEach(function (g) { validSlugs[String(g.slug || '').toLowerCase()] = true; });
    if (state.selectedGroup !== 'all' && state.selectedGroup !== 'unassigned' && !validSlugs[state.selectedGroup]) {
      state.selectedGroup = 'all';
    }

    var unassignedCnt = state.profiles.filter(function (p) { return !personGroupSlug(p); }).length;
    var activeGroupObj = state.peopleGroups.find(function (g) { return String(g.slug || '').toLowerCase() === state.selectedGroup; });

    var list = state.profiles.filter(function (p) {
      var gs = personGroupSlug(p);
      if (state.selectedGroup === 'unassigned' && gs) return false;
      if (state.selectedGroup !== 'all' && state.selectedGroup !== 'unassigned' && gs !== state.selectedGroup) return false;
      return !q || (p.identity + ' ' + (p.group || '') + ' ' + (p.address || '') + ' ' + (p.notes || '') + ' ' + (p.description || '')).toLowerCase().indexOf(q) !== -1;
    });

    var gq = state.peopleGroupSearch.trim().toLowerCase();
    var visibleGroups = state.peopleGroups.filter(function (g) {
      return !gq || (String(g.name || '') + ' ' + String(g.description || '')).toLowerCase().indexOf(gq) !== -1;
    });

    var folderChips =
      '<div class="rf-folder-search-wrap"><input class="rf-folder-search" id="rf-ppl-grp-search" type="search" placeholder="🔍 Search group folders…" value="' + esc(state.peopleGroupSearch) + '"/></div>' +
      '<div class="rf-folder-chip' + (state.selectedGroup === 'all' ? ' active' : '') + '" data-rf-grp="all">📂 All <span class="rf-fcnt">' + state.profiles.length + '</span></div>' +
      visibleGroups.map(function (g) {
        var gs = String(g.slug || '').toLowerCase();
        var cnt = state.profiles.filter(function (p) { return personGroupSlug(p) === gs; }).length;
        return '<div class="rf-folder-chip' + (state.selectedGroup === gs ? ' active' : '') + '" data-rf-grp="' + esc(gs) + '" title="' + esc(g.description || g.name) + '">' +
          '<span>📁 ' + esc(g.name) + '</span>' +
          '<span class="rf-fcnt">' + cnt + '</span>' +
          '<button type="button" class="rf-folder-act" data-rf-grp-menu="' + esc(gs) + '" aria-label="Folder options" title="Folder options">' + DOTS_SVG + '</button>' +
          '</div>';
      }).join('') +
      (!visibleGroups.length && gq ? '<span style="font-size:.78rem;color:rgb(var(--text-secondary))">No matching folders</span>' : '') +
      '<div class="rf-folder-chip' + (state.selectedGroup === 'unassigned' ? ' active' : '') + '" data-rf-grp="unassigned">📂 Unassigned <span class="rf-fcnt">' + unassignedCnt + '</span></div>';

    var groupOpts = function (curSlug) {
      return '<option value=""' + (!curSlug ? ' selected' : '') + '>📂 Unassigned</option>' +
        state.peopleGroups.map(function (g) {
          var gs = String(g.slug || '').toLowerCase();
          return '<option value="' + esc(gs) + '"' + (curSlug === gs ? ' selected' : '') + '>📁 ' + esc(g.name) + '</option>';
        }).join('');
    };

    body.innerHTML =
      '<div class="rf-section-head"><div><h2>👥 Enrolled People' + (activeGroupObj ? ' — 📁 ' + esc(activeGroupObj.name) : state.selectedGroup === 'unassigned' ? ' — Unassigned' : '') + '</h2>' +
      '<p>' + esc(activeGroupObj && activeGroupObj.description ? activeGroupObj.description : 'Organize enrolled people into maintainable group folders (e.g. Staff, External Entity). Use the ⋮ menu on any folder to rename or delete it.') + '</p></div>' +
      '<div style="display:flex;gap:8px;flex-wrap:wrap">' +
      '<button type="button" class="rf-btn" id="rf-add-group-btn">📁＋ New Group Folder</button>' +
      '<button type="button" class="rf-btn rf-btn-primary" id="rf-add-person-btn">＋ Add Person</button>' +
      '</div></div>' +
      '<div class="rf-folder-bar">' + folderChips + '</div>' +
      '<div class="rf-toolbar"><input class="rf-input" id="rf-ppl-search" style="flex:1;min-width:220px" type="search" placeholder="Search enrolled people in this folder…" value="' + esc(state.search) + '"/></div>' +
      '<div class="rf-grid">' +
      (list.length ? list.map(function (p) {
        var img = p.images && p.images[0] ? '<img class="rf-thumb" src="' + esc(apiUrl(p.images[0].url)) + '" alt="' + esc(p.identity) + '"/>' : '<div class="rf-thumb">👤</div>';
        var curGs = personGroupSlug(p);
        return '<article class="rf-card">' + img + '<div class="rf-card-body">' +
          '<h3 title="' + esc(p.identity) + '">' + esc(p.identity) + '</h3>' +
          '<p class="rf-desc">' + esc(p.notes || p.address || p.description || 'No notes') + '</p>' +
          '<div class="rf-meta">' + Number(p.vector_count || 0) + ' vector(s) · ' + (p.images ? p.images.length : 0) + ' photo(s)</div>' +
          '<div class="rf-actions">' +
          '<select class="rf-move-sel" data-rf-move-p="' + esc(p.slug) + '" title="Change group folder">' + groupOpts(curGs) + '</select>' +
          '<button type="button" class="rf-btn rf-btn-primary rf-btn-sm" data-rf-edit-p="' + esc(p.slug) + '">' + EDIT_SVG + '<span>Edit</span></button>' +
          '<button type="button" class="rf-btn rf-btn-danger rf-btn-sm" data-rf-del-p="' + esc(p.slug) + '">' + TRASH_SVG + '<span>Delete</span></button>' +
          '</div></div></article>';
      }).join('') : '<div class="rf-empty">No enrolled people in this folder yet. Move someone here using the folder dropdown or click "＋ Add Person".</div>') +
      '</div>';

    document.getElementById('rf-add-group-btn').onclick = function () { openGroupModal('people', null); };
    document.getElementById('rf-add-person-btn').onclick = function () { openPersonModal(null); };
    document.getElementById('rf-ppl-grp-search').oninput = function (e) {
      var pos = e.target.selectionStart;
      state.peopleGroupSearch = e.target.value;
      renderPeople(body);
      restoreFocus('rf-ppl-grp-search', pos);
    };
    document.getElementById('rf-ppl-search').oninput = function (e) {
      var pos = e.target.selectionStart;
      state.search = e.target.value;
      renderPeople(body);
      restoreFocus('rf-ppl-search', pos);
    };

    body.querySelectorAll('[data-rf-grp]').forEach(function (chip) {
      chip.onclick = function (ev) {
        if (ev.target.closest('.rf-folder-act')) return;
        state.selectedGroup = chip.getAttribute('data-rf-grp');
        renderPeople(body);
      };
    });
    body.querySelectorAll('[data-rf-grp-menu]').forEach(function (btn) {
      btn.onclick = function (ev) {
        ev.stopPropagation();
        var gs = btn.getAttribute('data-rf-grp-menu');
        var g = state.peopleGroups.find(function (x) { return String(x.slug || '').toLowerCase() === gs; });
        if (!g) return;
        var memberCnt = state.profiles.filter(function (p) { return personGroupSlug(p) === gs; }).length;
        openFolderMenu(btn, {
          onRename: function () { openGroupModal('people', g); },
          onDelete: function () {
            openConfirmModal({
              title: 'Delete Group Folder "' + g.name + '"?',
              message: 'This removes the "' + g.name + '" folder. The ' + memberCnt + ' enrolled ' + (memberCnt === 1 ? 'person' : 'people') + ' inside will NOT be deleted — they will move safely to Unassigned.',
              confirmLabel: 'Delete Folder',
              onConfirm: async function () {
                var r = await api('/admin/api/people-groups/' + encodeURIComponent(g.slug), { method: 'DELETE' });
                if (r.ok) {
                  if (state.selectedGroup === gs) state.selectedGroup = 'all';
                  toast('Deleted folder "' + g.name + '"');
                  await refreshAll(false);
                } else toast((r.data && r.data.reason) || 'Delete failed', false);
              }
            });
          }
        });
      };
    });
    body.querySelectorAll('[data-rf-move-p]').forEach(function (sel) {
      sel.onchange = async function () {
        var slug = sel.getAttribute('data-rf-move-p');
        var nextGrp = sel.value;
        var r = await api('/admin/api/profiles/' + encodeURIComponent(slug), {
          method: 'PATCH',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ group_slug: nextGrp })
        });
        if (r.ok) {
          toast(nextGrp ? 'Moved to folder' : 'Moved to Unassigned');
          await refreshAll(false);
        } else {
          toast((r.data && r.data.reason) || 'Move failed', false);
          renderPeople(body);
        }
      };
    });
    body.querySelectorAll('[data-rf-edit-p]').forEach(function (b) {
      b.onclick = function () {
        var p = state.profiles.find(function (x) { return x.slug === b.getAttribute('data-rf-edit-p'); });
        openPersonModal(p || null);
      };
    });
    body.querySelectorAll('[data-rf-del-p]').forEach(function (b) {
      b.onclick = function () {
        var p = state.profiles.find(function (x) { return x.slug === b.getAttribute('data-rf-del-p'); });
        if (!p) return;
        var vc = Number(p.vector_count || 0);
        var ic = p.images ? p.images.length : 0;
        openConfirmModal({
          title: 'Delete "' + p.identity + '"?',
          message: 'Delete ' + p.identity + ' and their ' + vc + ' vector(s) and ' + ic + ' photo(s)? This cannot be undone.',
          confirmLabel: 'Delete Person',
          onConfirm: async function () {
            var r = await api('/admin/api/profiles/' + encodeURIComponent(p.slug), { method: 'DELETE' });
            if (r.ok) { toast('Deleted ' + p.identity); await refreshAll(false); }
            else toast((r.data && r.data.reason) || 'Delete failed', false);
          }
        });
      };
    });
  }

  function openGroupModal(kind, group) {
    var isDoc = kind === 'document';
    var isLoc = kind === 'location';
    var endpoint = isDoc
      ? '/admin/api/document-groups'
      : isLoc
        ? '/admin/api/location-groups'
        : '/admin/api/people-groups';
    var placeholder = isDoc
      ? 'e.g. Historical Manuscripts, Official Records, Treaties, Letters'
      : isLoc
        ? 'e.g. Arkib Negara Malaysia, Historical Landmarks, State Branches'
        : 'e.g. Staff, External Entity, VIP Guests, Researchers';
    var newTitle = isDoc
      ? 'New Document Group Folder'
      : isLoc
        ? 'New Location Group Folder'
        : 'New People Group Folder';
    var wrap = document.createElement('div');
    wrap.className = 'rf-modal-backdrop';
    wrap.innerHTML =
      '<div class="rf-modal" style="width:min(520px,100%)"><h2>' + (group ? 'Rename Group Folder' : newTitle) + '</h2>' +
      '<form id="rf-g-form"><div class="rf-form-grid">' +
      '<div class="rf-field full"><label>Group Folder Name</label><input class="rf-input" id="rf-g-name" required placeholder="' + placeholder + '" value="' + esc(group ? group.name || '' : '') + '"/></div>' +
      '<div class="rf-field full"><label>Description</label><textarea class="rf-textarea" id="rf-g-desc" placeholder="Optional description of what belongs in this folder…">' + esc(group ? group.description || '' : '') + '</textarea></div>' +
      '</div><div class="rf-actions" style="justify-content:flex-end;margin-top:16px">' +
      '<button type="button" class="rf-btn" id="rf-g-cancel">Cancel</button>' +
      '<button type="submit" class="rf-btn rf-btn-primary">' + (group ? 'Save Rename' : 'Create Folder') + '</button>' +
      '</div></form></div>';
    document.body.appendChild(wrap);
    wrap.onclick = function (e) { if (e.target === wrap) wrap.remove(); };
    document.getElementById('rf-g-cancel').onclick = function () { wrap.remove(); };
    document.getElementById('rf-g-form').onsubmit = async function (ev) {
      ev.preventDefault();
      var name = document.getElementById('rf-g-name').value.trim();
      var description = document.getElementById('rf-g-desc').value.trim();
      var r = await api(group ? endpoint + '/' + encodeURIComponent(group.slug) : endpoint, {
        method: group ? 'PATCH' : 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name: name, description: description })
      });
      if (r.ok) {
        if (group && r.data && r.data.group) {
          var nextSlug = String(r.data.group.slug || '').toLowerCase();
          var oldSlug = String(group.slug || '').toLowerCase();
          if (isDoc && state.selectedDocGroup === oldSlug) state.selectedDocGroup = nextSlug;
          if (isLoc && state.selectedLocGroup === oldSlug) state.selectedLocGroup = nextSlug;
          if (!isDoc && !isLoc && state.selectedGroup === oldSlug) state.selectedGroup = nextSlug;
        }
        wrap.remove();
        toast(group ? 'Renamed group folder to "' + name + '"' : 'Created group folder "' + name + '"');
        await refreshAll(false);
      } else toast((r.data && r.data.reason) || 'Could not save group folder', false);
    };
  }

  function openPersonModal(profile) {
    state.editingProfile = profile;
    state.selectedPhoto = null;
    var wrap = document.createElement('div');
    wrap.className = 'rf-modal-backdrop';
    var pubRows = [], privRows = [];
    if (profile && profile.other_details && typeof profile.other_details === 'object') {
      Object.keys(profile.other_details).forEach(function (k) {
        var v = profile.other_details[k];
        var val = v && typeof v === 'object' ? (v.value || '') : String(v || '');
        var vis = v && typeof v === 'object' ? (v.visibility || 'public') : 'public';
        (vis === 'private' ? privRows : pubRows).push([k, val]);
      });
    }
    var rowHtml = function (k, v, vis) {
      return '<div class="rf-kv-row" data-vis="' + vis + '">' +
        '<input class="rf-input rf-kv-k" style="padding:6px 8px;font-size:.8rem" placeholder="Field" value="' + esc(k) + '"/>' +
        '<input class="rf-input rf-kv-v" style="padding:6px 8px;font-size:.8rem" placeholder="Value" value="' + esc(v) + '"/>' +
        '<button type="button" class="rf-btn rf-btn-sm rf-kv-swap" title="Toggle public/private">⇄</button>' +
        '<button type="button" class="rf-icon-del rf-kv-rm" title="Delete detail">' + TRASH_SVG + '</button></div>';
    };

    var defaultGrp = profile ? personGroupSlug(profile) : (state.selectedGroup !== 'all' && state.selectedGroup !== 'unassigned' ? state.selectedGroup : '');
    var grpSelectHtml =
      '<option value=""' + (!defaultGrp ? ' selected' : '') + '>Unassigned</option>' +
      state.peopleGroups.map(function (g) {
        var gs = String(g.slug || '').toLowerCase();
        return '<option value="' + esc(gs) + '"' + (defaultGrp === gs ? ' selected' : '') + '>' + esc(g.name) + '</option>';
      }).join('');

    wrap.innerHTML =
      '<div class="rf-modal"><h2>' + (profile ? 'Edit Person — ' + esc(profile.identity) : 'Add Enrolled Person') + '</h2>' +
      '<form id="rf-p-form"><div class="rf-form-grid">' +
      '<div class="rf-field"><label>Name</label><input class="rf-input" id="rf-p-name" required value="' + esc(profile ? profile.identity : '') + '"/></div>' +
      '<div class="rf-field"><label>Group Folder</label><select class="rf-select" id="rf-p-group">' + grpSelectHtml + '</select></div>' +
      (!profile ? '<div class="rf-field full"><label>Face Photo (required)</label><input class="rf-input" id="rf-p-img" type="file" accept=".jpg,.jpeg,.png,.webp,.bmp" required/></div>' : '') +
      (profile ? '<div class="rf-field full"><label>Stored Photos (click to view/edit details; hover for × delete)</label><div class="rf-gallery" id="rf-p-gal"></div>' +
        '<div id="rf-photo-meta" class="rf-box" style="display:none;margin-top:8px"></div>' +
        '<input class="rf-input" id="rf-p-addimg" type="file" accept=".jpg,.jpeg,.png,.webp,.bmp" style="margin-top:8px"/></div>' : '') +
      '<div class="rf-field full"><label>Address</label><input class="rf-input" id="rf-p-addr" value="' + esc(profile ? profile.address || '' : '') + '"/></div>' +
      '<div class="rf-field full"><label>Notes</label><textarea class="rf-textarea" id="rf-p-notes">' + esc(profile ? profile.notes || '' : '') + '</textarea></div>' +
      '<div class="rf-field full"><label>Custom Details (Public shown in chat · Private Admin-only)</label>' +
      '<div class="rf-kv-split">' +
      '<div class="rf-kv-col"><div style="display:flex;justify-content:space-between;align-items:center"><b style="font-size:.78rem">🌐 Public Data</b><button type="button" class="rf-btn rf-btn-ok rf-btn-sm" id="rf-add-pub">＋ Add</button></div><div id="rf-pub-list">' + pubRows.map(function (r) { return rowHtml(r[0], r[1], 'public'); }).join('') + '</div></div>' +
      '<div class="rf-kv-col"><div style="display:flex;justify-content:space-between;align-items:center"><b style="font-size:.78rem">🔒 Private Data</b><button type="button" class="rf-btn rf-btn-danger rf-btn-sm" id="rf-add-priv">＋ Add</button></div><div id="rf-priv-list">' + privRows.map(function (r) { return rowHtml(r[0], r[1], 'private'); }).join('') + '</div></div>' +
      '</div></div></div>' +
      '<div class="rf-actions" style="justify-content:flex-end;margin-top:18px">' +
      '<button type="button" class="rf-btn" id="rf-p-cancel">Cancel</button>' +
      '<button type="submit" class="rf-btn rf-btn-primary" id="rf-p-save">Save</button>' +
      '</div></form></div>';

    document.body.appendChild(wrap);
    wrap.addEventListener('click', function (e) { if (e.target === wrap) wrap.remove(); });
    document.getElementById('rf-p-cancel').addEventListener('click', function () { wrap.remove(); });

    function bindKvEvents(container) {
      container.querySelectorAll('.rf-kv-rm').forEach(function (b) {
        b.onclick = function () { b.closest('.rf-kv-row').remove(); };
      });
      container.querySelectorAll('.rf-kv-swap').forEach(function (b) {
        b.onclick = function () {
          var row = b.closest('.rf-kv-row');
          var next = row.getAttribute('data-vis') === 'public' ? 'private' : 'public';
          row.setAttribute('data-vis', next);
          document.getElementById(next === 'public' ? 'rf-pub-list' : 'rf-priv-list').appendChild(row);
        };
      });
    }
    bindKvEvents(wrap);
    document.getElementById('rf-add-pub').onclick = function () {
      var list = document.getElementById('rf-pub-list');
      list.insertAdjacentHTML('beforeend', rowHtml('', '', 'public'));
      bindKvEvents(list);
    };
    document.getElementById('rf-add-priv').onclick = function () {
      var list = document.getElementById('rf-priv-list');
      list.insertAdjacentHTML('beforeend', rowHtml('', '', 'private'));
      bindKvEvents(list);
    };

    function renderGal(cur) {
      var gal = document.getElementById('rf-p-gal');
      if (!gal) return;
      var imgs = cur.images || [];
      gal.innerHTML = imgs.length ? imgs.map(function (im) {
        return '<div class="rf-tile' + (state.selectedPhoto === im.filename ? ' selected' : '') + '" data-fn="' + esc(im.filename) + '">' +
          '<img src="' + esc(apiUrl(im.url)) + '" alt="' + esc(im.filename) + '"/>' +
          '<button type="button" class="rf-tile-del" data-del="' + esc(im.filename) + '" title="Delete photo">' + TRASH_SVG + '</button></div>';
      }).join('') : '<div style="font-size:.78rem;color:rgb(var(--text-secondary));padding:10px">No stored photos yet.</div>';

      gal.querySelectorAll('.rf-tile').forEach(function (t) {
        t.onclick = function (e) {
          if (e.target.classList.contains('rf-tile-del')) return;
          var fn = t.getAttribute('data-fn');
          var im = (cur.images || []).find(function (x) { return x.filename === fn; });
          if (!im) return;
          state.selectedPhoto = fn;
          renderGal(cur);
          var box = document.getElementById('rf-photo-meta');
          box.style.display = 'block';
          box.innerHTML =
            '<div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:8px"><b>Photo: ' + esc(fn) + '</b>' +
            '<div style="display:flex;gap:6px"><button type="button" class="rf-btn rf-btn-sm" id="rf-pm-zoom">🔍 Enlarge</button>' +
            '<button type="button" class="rf-btn rf-btn-ok rf-btn-sm" id="rf-pm-save">Save photo details</button></div></div>' +
            '<div class="rf-form-grid">' +
            '<div class="rf-field full"><label>Description</label><input class="rf-input" id="rf-pm-desc" value="' + esc(im.description || '') + '"/></div>' +
            '<div class="rf-field"><label>Photograph by</label><input class="rf-input" id="rf-pm-by" value="' + esc(im.photograph_by || '') + '"/></div>' +
            '<div class="rf-field"><label>Location</label><input class="rf-input" id="rf-pm-loc" value="' + esc(im.location || '') + '"/></div>' +
            '<div class="rf-field"><label>Date</label><input class="rf-input" id="rf-pm-date" type="date" value="' + esc(im.date || '') + '"/></div>' +
            '<div class="rf-field"><label>Time</label><input class="rf-input" id="rf-pm-time" type="time" value="' + esc(im.time || '') + '"/></div>' +
            '</div>';
          document.getElementById('rf-pm-zoom').onclick = function () { openViewer(apiUrl(im.url)); };
          document.getElementById('rf-pm-save').onclick = async function () {
            var r = await api('/admin/api/profiles/' + encodeURIComponent(cur.slug) + '/images/' + encodeURIComponent(fn), {
              method: 'PATCH',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({
                description: document.getElementById('rf-pm-desc').value,
                photograph_by: document.getElementById('rf-pm-by').value,
                location: document.getElementById('rf-pm-loc').value,
                date: document.getElementById('rf-pm-date').value,
                time: document.getElementById('rf-pm-time').value
              })
            });
            if (r.ok && r.data.profile) {
              cur = r.data.profile;
              renderGal(cur);
              toast('Photo details saved');
              refreshAll(false);
            } else toast(r.data.reason || 'Save failed', false);
          };
        };
      });
      gal.querySelectorAll('.rf-tile-del').forEach(function (d) {
        d.onclick = function (e) {
          e.stopPropagation();
          var fn = d.getAttribute('data-del');
          openConfirmModal({
            title: 'Delete Photo?',
            message: 'Delete photo "' + fn + '" from ' + cur.identity + '? This cannot be undone.',
            confirmLabel: 'Delete Photo',
            onConfirm: async function () {
              var r = await api('/admin/api/profiles/' + encodeURIComponent(cur.slug) + '/images/' + encodeURIComponent(fn), { method: 'DELETE' });
              if (r.ok && r.data.profile) {
                cur = r.data.profile;
                renderGal(cur);
                toast('Photo deleted');
                refreshAll(false);
              } else toast((r.data && r.data.reason) || 'Delete failed', false);
            }
          });
        };
      });
    }

    if (profile) {
      renderGal(profile);
      document.getElementById('rf-p-addimg').onchange = async function (e) {
        if (!e.target.files[0]) return;
        var fd = new FormData();
        fd.append('image', e.target.files[0]);
        var r = await api('/admin/api/profiles/' + encodeURIComponent(profile.slug) + '/images', { method: 'POST', body: fd });
        e.target.value = '';
        if (r.ok && r.data.profile) {
          profile = r.data.profile;
          renderGal(profile);
          toast('Photo added');
          refreshAll(false);
        } else toast(r.data.reason || 'Upload failed', false);
      };
    }

    document.getElementById('rf-p-form').onsubmit = async function (ev) {
      ev.preventDefault();
      var other = {};
      wrap.querySelectorAll('.rf-kv-row').forEach(function (row) {
        var k = row.querySelector('.rf-kv-k').value.trim();
        var v = row.querySelector('.rf-kv-v').value.trim();
        if (k) other[k] = { value: v, visibility: row.getAttribute('data-vis') || 'public' };
      });
      var saveBtn = document.getElementById('rf-p-save');
      saveBtn.disabled = true;
      try {
        var r;
        if (profile) {
          r = await api('/admin/api/profiles/' + encodeURIComponent(profile.slug), {
            method: 'PATCH',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
              identity: document.getElementById('rf-p-name').value,
              group_slug: document.getElementById('rf-p-group').value,
              address: document.getElementById('rf-p-addr').value,
              notes: document.getElementById('rf-p-notes').value,
              other_details: other
            })
          });
        } else {
          var fd = new FormData();
          fd.append('identity', document.getElementById('rf-p-name').value);
          fd.append('group_slug', document.getElementById('rf-p-group').value);
          fd.append('address', document.getElementById('rf-p-addr').value);
          fd.append('notes', document.getElementById('rf-p-notes').value);
          if (Object.keys(other).length) fd.append('other_details', JSON.stringify(other));
          fd.append('image', document.getElementById('rf-p-img').files[0]);
          r = await api('/admin/api/profiles', { method: 'POST', body: fd });
        }
        if (!r.ok) throw new Error((r.data && r.data.reason) || 'Could not save');
        wrap.remove();
        toast(profile ? 'Profile updated' : 'Person enrolled');
        await refreshAll(false);
      } catch (err) {
        toast('Error: ' + err.message, false);
        saveBtn.disabled = false;
      }
    };
  }

  // ─── Tab 3: Archival Locations (with Maintainable Group Folders) ──────────
  function locGroupSlug(l) {
    return String(l.group_slug || '').trim().toLowerCase();
  }

  function renderLocations(body) {
    closeMenus();
    var q = state.search.toLowerCase();
    var validSlugs = {};
    state.locationGroups.forEach(function (g) { validSlugs[String(g.slug || '').toLowerCase()] = true; });
    if (state.selectedLocGroup !== 'all' && state.selectedLocGroup !== 'unassigned' && !validSlugs[state.selectedLocGroup]) {
      state.selectedLocGroup = 'all';
    }

    var unassignedCnt = state.locations.filter(function (l) { return !locGroupSlug(l); }).length;
    var activeGroupObj = state.locationGroups.find(function (g) { return String(g.slug || '').toLowerCase() === state.selectedLocGroup; });

    var list = state.locations.filter(function (l) {
      var gs = locGroupSlug(l);
      if (state.selectedLocGroup === 'unassigned' && gs) return false;
      if (state.selectedLocGroup !== 'all' && state.selectedLocGroup !== 'unassigned' && gs !== state.selectedLocGroup) return false;
      return !q || JSON.stringify(l).toLowerCase().indexOf(q) !== -1;
    });

    var gq = state.locGroupSearch.trim().toLowerCase();
    var visibleGroups = state.locationGroups.filter(function (g) {
      return !gq || (String(g.name || '') + ' ' + String(g.description || '')).toLowerCase().indexOf(gq) !== -1;
    });

    var folderChips =
      '<div class="rf-folder-search-wrap"><input class="rf-folder-search" id="rf-loc-grp-search" type="search" placeholder="🔍 Search group folders…" value="' + esc(state.locGroupSearch) + '"/></div>' +
      '<div class="rf-folder-chip' + (state.selectedLocGroup === 'all' ? ' active' : '') + '" data-rf-lgrp="all">📂 All <span class="rf-fcnt">' + state.locations.length + '</span></div>' +
      visibleGroups.map(function (g) {
        var gs = String(g.slug || '').toLowerCase();
        var cnt = state.locations.filter(function (l) { return locGroupSlug(l) === gs; }).length;
        return '<div class="rf-folder-chip' + (state.selectedLocGroup === gs ? ' active' : '') + '" data-rf-lgrp="' + esc(gs) + '" title="' + esc(g.description || g.name) + '">' +
          '<span>📁 ' + esc(g.name) + '</span>' +
          '<span class="rf-fcnt">' + cnt + '</span>' +
          '<button type="button" class="rf-folder-act" data-rf-lgrp-menu="' + esc(gs) + '" aria-label="Folder options" title="Folder options">' + DOTS_SVG + '</button>' +
          '</div>';
      }).join('') +
      (!visibleGroups.length && gq ? '<span style="font-size:.78rem;color:rgb(var(--text-secondary))">No matching folders</span>' : '') +
      '<div class="rf-folder-chip' + (state.selectedLocGroup === 'unassigned' ? ' active' : '') + '" data-rf-lgrp="unassigned">📂 Unassigned <span class="rf-fcnt">' + unassignedCnt + '</span></div>';

    var groupOpts = function (curSlug) {
      return '<option value=""' + (!curSlug ? ' selected' : '') + '>📂 Unassigned</option>' +
        state.locationGroups.map(function (g) {
          var gs = String(g.slug || '').toLowerCase();
          return '<option value="' + esc(gs) + '"' + (curSlug === gs ? ' selected' : '') + '>📁 ' + esc(g.name) + '</option>';
        }).join('');
    };

    body.innerHTML =
      '<div class="rf-section-head"><div><h2>📍 Archival Locations &amp; Landmarks' + (activeGroupObj ? ' — 📁 ' + esc(activeGroupObj.name) : state.selectedLocGroup === 'unassigned' ? ' — Unassigned' : '') + '</h2>' +
      '<p>' + esc(activeGroupObj && activeGroupObj.description ? activeGroupObj.description : 'Organize halls, wings, rooms, and buildings into maintainable group folders (e.g. Arkib Negara Malaysia, Historical Landmarks). Use the ⋮ menu on any folder to rename or delete it.') + '</p></div>' +
      '<div style="display:flex;gap:8px;flex-wrap:wrap">' +
      '<button type="button" class="rf-btn" id="rf-add-lgrp-btn">📁＋ New Group Folder</button>' +
      '<button type="button" class="rf-btn rf-btn-primary" id="rf-add-loc-btn">＋ Add Location</button>' +
      '</div></div>' +
      '<div class="rf-folder-bar">' + folderChips + '</div>' +
      '<div class="rf-toolbar"><input class="rf-input" id="rf-loc-search" style="flex:1;min-width:220px" type="search" placeholder="Search locations in this folder by name, wing, hall, or address…" value="' + esc(state.search) + '"/></div>' +
      '<div class="rf-grid">' +
      (list.length ? list.map(function (l) {
        var thumb = l.images && l.images[0] ? '<img class="rf-thumb" src="' + esc(apiUrl(l.images[0].admin_url)) + '" alt="' + esc(l.name) + '"/>' : '<div class="rf-thumb">📍</div>';
        var sub = [l.building_or_wing, l.floor_or_room].filter(Boolean).join(' · ') || l.parent_institution || l.category || 'Archival Location';
        var curGs = locGroupSlug(l);
        return '<article class="rf-card">' + thumb + '<div class="rf-card-body">' +
          '<h3 title="' + esc(l.name) + '">' + esc(l.name) + '</h3>' +
          '<div class="rf-sub" title="' + esc(sub) + '">' + esc(sub) + '</div>' +
          '<p class="rf-desc">' + esc(l.description || l.historical_significance || 'No description yet.') + '</p>' +
          '<div class="rf-meta">' + (l.image_count || 0) + ' photo(s)' + (l.coordinates ? ' · ' + esc(l.coordinates) : '') + '</div>' +
          '<div class="rf-actions">' +
          '<select class="rf-move-sel" data-rf-move-l="' + esc(l.slug) + '" title="Change group folder">' + groupOpts(curGs) + '</select>' +
          '<button type="button" class="rf-btn rf-btn-primary rf-btn-sm" data-rf-edit-l="' + esc(l.slug) + '">' + EDIT_SVG + '<span>Edit</span></button>' +
          '<button type="button" class="rf-btn rf-btn-danger rf-btn-sm" data-rf-del-l="' + esc(l.slug) + '">' + TRASH_SVG + '<span>Delete</span></button>' +
          '</div></div></article>';
      }).join('') : '<div class="rf-empty">No archival locations in this folder yet. Move one here using the folder dropdown or click "＋ Add Location".</div>') +
      '</div>';

    document.getElementById('rf-add-lgrp-btn').onclick = function () { openGroupModal('location', null); };
    document.getElementById('rf-add-loc-btn').onclick = function () { openLocationModal(null); };
    document.getElementById('rf-loc-grp-search').oninput = function (e) {
      var pos = e.target.selectionStart;
      state.locGroupSearch = e.target.value;
      renderLocations(body);
      restoreFocus('rf-loc-grp-search', pos);
    };
    document.getElementById('rf-loc-search').oninput = function (e) {
      var pos = e.target.selectionStart;
      state.search = e.target.value;
      renderLocations(body);
      restoreFocus('rf-loc-search', pos);
    };

    body.querySelectorAll('[data-rf-lgrp]').forEach(function (chip) {
      chip.onclick = function (ev) {
        if (ev.target.closest('.rf-folder-act')) return;
        state.selectedLocGroup = chip.getAttribute('data-rf-lgrp');
        renderLocations(body);
      };
    });
    body.querySelectorAll('[data-rf-lgrp-menu]').forEach(function (btn) {
      btn.onclick = function (ev) {
        ev.stopPropagation();
        var gs = btn.getAttribute('data-rf-lgrp-menu');
        var g = state.locationGroups.find(function (x) { return String(x.slug || '').toLowerCase() === gs; });
        if (!g) return;
        var memberCnt = state.locations.filter(function (l) { return locGroupSlug(l) === gs; }).length;
        openFolderMenu(btn, {
          onRename: function () { openGroupModal('location', g); },
          onDelete: function () {
            openConfirmModal({
              title: 'Delete Group Folder "' + g.name + '"?',
              message: 'This removes the "' + g.name + '" folder. The ' + memberCnt + ' location' + (memberCnt === 1 ? '' : 's') + ' inside will NOT be deleted — they will move safely to Unassigned.',
              confirmLabel: 'Delete Folder',
              onConfirm: async function () {
                var r = await api('/admin/api/location-groups/' + encodeURIComponent(g.slug), { method: 'DELETE' });
                if (r.ok) {
                  if (state.selectedLocGroup === gs) state.selectedLocGroup = 'all';
                  toast('Deleted folder "' + g.name + '"');
                  await refreshAll(false);
                } else toast((r.data && r.data.reason) || 'Delete failed', false);
              }
            });
          }
        });
      };
    });
    body.querySelectorAll('[data-rf-move-l]').forEach(function (sel) {
      sel.onchange = async function () {
        var slug = sel.getAttribute('data-rf-move-l');
        var nextGrp = sel.value;
        var r = await api('/admin/api/locations/' + encodeURIComponent(slug), {
          method: 'PATCH',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ group_slug: nextGrp })
        });
        if (r.ok) {
          toast(nextGrp ? 'Moved to folder' : 'Moved to Unassigned');
          await refreshAll(false);
        } else {
          toast((r.data && r.data.reason) || 'Move failed', false);
          renderLocations(body);
        }
      };
    });
    body.querySelectorAll('[data-rf-edit-l]').forEach(function (b) {
      b.onclick = function () {
        var l = state.locations.find(function (x) { return x.slug === b.getAttribute('data-rf-edit-l'); });
        openLocationModal(l || null);
      };
    });
    body.querySelectorAll('[data-rf-del-l]').forEach(function (b) {
      b.onclick = function () {
        var l = state.locations.find(function (x) { return x.slug === b.getAttribute('data-rf-del-l'); });
        if (!l) return;
        openConfirmModal({
          title: 'Delete Location "' + l.name + '"?',
          message: 'Delete location "' + l.name + '" and its ' + (l.image_count || 0) + ' reference photo(s)? This cannot be undone.',
          confirmLabel: 'Delete Location',
          onConfirm: async function () {
            var r = await api('/admin/api/locations/' + encodeURIComponent(l.slug), { method: 'DELETE' });
            if (r.ok) { toast('Deleted location'); await refreshAll(false); }
            else toast((r.data && r.data.reason) || 'Delete failed', false);
          }
        });
      };
    });
  }

  function openLocationModal(loc) {
    var wrap = document.createElement('div');
    wrap.className = 'rf-modal-backdrop';
    var defaultGrp = loc ? locGroupSlug(loc) : (state.selectedLocGroup !== 'all' && state.selectedLocGroup !== 'unassigned' ? state.selectedLocGroup : '');
    var grpSelectHtml =
      '<option value=""' + (!defaultGrp ? ' selected' : '') + '>Unassigned</option>' +
      state.locationGroups.map(function (g) {
        var gs = String(g.slug || '').toLowerCase();
        return '<option value="' + esc(gs) + '"' + (defaultGrp === gs ? ' selected' : '') + '>' + esc(g.name) + '</option>';
      }).join('');
    wrap.innerHTML =
      '<div class="rf-modal"><h2>' + (loc ? 'Edit Location' : 'Add Archival Location') + '</h2>' +
      '<form id="rf-l-form"><div class="rf-form-grid">' +
      '<div class="rf-field"><label>Location Name</label><input class="rf-input" id="rf-l-name" required value="' + esc(loc ? loc.name || '' : '') + '"/></div>' +
      '<div class="rf-field"><label>Group Folder</label><select class="rf-select" id="rf-l-group">' + grpSelectHtml + '</select></div>' +
      '<div class="rf-field"><label>Parent Institution</label><input class="rf-input" id="rf-l-inst" value="' + esc(loc ? loc.parent_institution || '' : '') + '"/></div>' +
      '<div class="rf-field"><label>Category</label><input class="rf-input" id="rf-l-cat" value="' + esc(loc ? loc.category || '' : '') + '"/></div>' +
      '<div class="rf-field"><label>Building / Wing</label><input class="rf-input" id="rf-l-bld" value="' + esc(loc ? loc.building_or_wing || '' : '') + '"/></div>' +
      '<div class="rf-field"><label>Floor / Room</label><input class="rf-input" id="rf-l-flr" value="' + esc(loc ? loc.floor_or_room || '' : '') + '"/></div>' +
      '<div class="rf-field full"><label>Address</label><input class="rf-input" id="rf-l-addr" value="' + esc(loc ? loc.address || '' : '') + '"/></div>' +
      '<div class="rf-field full"><label>GPS Coordinates</label><input class="rf-input" id="rf-l-gps" value="' + esc(loc ? loc.coordinates || '' : '') + '"/></div>' +
      '<div class="rf-field full"><label>Historical Significance</label><textarea class="rf-textarea" id="rf-l-hist">' + esc(loc ? loc.historical_significance || '' : '') + '</textarea></div>' +
      '<div class="rf-field full"><label>Description</label><textarea class="rf-textarea" id="rf-l-desc">' + esc(loc ? loc.description || '' : '') + '</textarea></div>' +
      '<div class="rf-field full"><label>Notes</label><textarea class="rf-textarea" id="rf-l-notes">' + esc(loc ? loc.notes || '' : '') + '</textarea></div>' +
      (loc ? '<div class="rf-field full"><label>Reference Photos (CLIP-indexed · duplicate-checked)</label><div class="rf-gallery" id="rf-l-gal"></div><input class="rf-input" id="rf-l-addimg" type="file" accept=".jpg,.jpeg,.png,.webp,.bmp" style="margin-top:8px"/></div>' : '') +
      '</div><div class="rf-actions" style="justify-content:flex-end;margin-top:16px">' +
      '<button type="button" class="rf-btn" id="rf-l-cancel">Cancel</button>' +
      '<button type="submit" class="rf-btn rf-btn-primary">Save Location</button></div></form></div>';

    document.body.appendChild(wrap);
    wrap.onclick = function (e) { if (e.target === wrap) wrap.remove(); };
    document.getElementById('rf-l-cancel').onclick = function () { wrap.remove(); };

    function renderLocGal(cur) {
      var gal = document.getElementById('rf-l-gal');
      if (!gal) return;
      var imgs = cur.images || [];
      gal.innerHTML = imgs.length ? imgs.map(function (im) {
        return '<div class="rf-tile"><img src="' + esc(apiUrl(im.admin_url)) + '" data-zoom="' + esc(apiUrl(im.admin_url)) + '"/><button type="button" class="rf-tile-del" data-del="' + esc(im.filename) + '" title="Delete photo">' + TRASH_SVG + '</button></div>';
      }).join('') : '<div style="font-size:.78rem;color:rgb(var(--text-secondary));padding:10px">No reference photos yet.</div>';
      gal.querySelectorAll('[data-zoom]').forEach(function (img) { img.onclick = function () { openViewer(img.getAttribute('data-zoom')); }; });
      gal.querySelectorAll('[data-del]').forEach(function (btn) {
        btn.onclick = function (e) {
          e.stopPropagation();
          var fn = btn.getAttribute('data-del');
          openConfirmModal({
            title: 'Delete Reference Photo?',
            message: 'Delete reference photo "' + fn + '" from ' + cur.name + '?',
            confirmLabel: 'Delete Photo',
            onConfirm: async function () {
              var r = await api('/admin/api/locations/' + encodeURIComponent(cur.slug) + '/images/' + encodeURIComponent(fn), { method: 'DELETE' });
              if (r.ok && r.data.location) { cur = r.data.location; renderLocGal(cur); toast('Photo deleted'); refreshAll(false); }
            }
          });
        };
      });
    }

    if (loc) {
      renderLocGal(loc);
      var upInput = document.getElementById('rf-l-addimg');
      async function doLocUpload(file, force) {
        var fd = new FormData();
        fd.append('image', file);
        var r = await api('/admin/api/locations/' + encodeURIComponent(loc.slug) + '/images' + (force ? '?force=1' : ''), { method: 'POST', body: fd });
        if (r.status === 409 && !force) {
          openConfirmModal({
            title: 'Duplicate Image Detected',
            message: (r.data.reason || 'This image already exists in the archive.') + ' Do you still want to upload a duplicate copy?',
            confirmLabel: 'Upload Anyway',
            primary: true,
            onConfirm: function () { return doLocUpload(file, true); },
            onCancel: function () { toast('Duplicate upload prevented', false); }
          });
          return;
        }
        if (r.ok && r.data.location) { loc = r.data.location; renderLocGal(loc); toast('Photo uploaded & indexed'); refreshAll(false); }
        else toast((r.data && r.data.reason) || 'Upload failed', false);
      }
      upInput.onchange = function () { if (upInput.files[0]) { doLocUpload(upInput.files[0], false); upInput.value = ''; } };
    }

    document.getElementById('rf-l-form').onsubmit = async function (ev) {
      ev.preventDefault();
      var payload = {
        name: document.getElementById('rf-l-name').value,
        group_slug: document.getElementById('rf-l-group').value,
        parent_institution: document.getElementById('rf-l-inst').value,
        category: document.getElementById('rf-l-cat').value,
        building_or_wing: document.getElementById('rf-l-bld').value,
        floor_or_room: document.getElementById('rf-l-flr').value,
        address: document.getElementById('rf-l-addr').value,
        coordinates: document.getElementById('rf-l-gps').value,
        historical_significance: document.getElementById('rf-l-hist').value,
        description: document.getElementById('rf-l-desc').value,
        notes: document.getElementById('rf-l-notes').value
      };
      var r = await api(loc ? '/admin/api/locations/' + encodeURIComponent(loc.slug) : '/admin/api/locations', {
        method: loc ? 'PATCH' : 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload)
      });
      if (r.ok) { wrap.remove(); toast(loc ? 'Location updated' : 'Location created'); await refreshAll(false); }
      else toast((r.data && r.data.reason) || 'Save failed', false);
    };
  }

  // ─── Tab 4: Archival Documents (with Maintainable Group Folders) ──────────
  function docGroupSlug(d) {
    return String(d.group_slug || '').trim().toLowerCase();
  }

  function renderDocuments(body) {
    closeMenus();
    var q = state.search.toLowerCase();
    var validSlugs = {};
    state.documentGroups.forEach(function (g) { validSlugs[String(g.slug || '').toLowerCase()] = true; });
    if (state.selectedDocGroup !== 'all' && state.selectedDocGroup !== 'unassigned' && !validSlugs[state.selectedDocGroup]) {
      state.selectedDocGroup = 'all';
    }

    var unassignedCnt = state.documents.filter(function (d) { return !docGroupSlug(d); }).length;
    var activeGroupObj = state.documentGroups.find(function (g) { return String(g.slug || '').toLowerCase() === state.selectedDocGroup; });

    var list = state.documents.filter(function (d) {
      var gs = docGroupSlug(d);
      if (state.selectedDocGroup === 'unassigned' && gs) return false;
      if (state.selectedDocGroup !== 'all' && state.selectedDocGroup !== 'unassigned' && gs !== state.selectedDocGroup) return false;
      return !q || JSON.stringify(d).toLowerCase().indexOf(q) !== -1;
    });

    var gq = state.docGroupSearch.trim().toLowerCase();
    var visibleGroups = state.documentGroups.filter(function (g) {
      return !gq || (String(g.name || '') + ' ' + String(g.description || '')).toLowerCase().indexOf(gq) !== -1;
    });

    var folderChips =
      '<div class="rf-folder-search-wrap"><input class="rf-folder-search" id="rf-doc-grp-search" type="search" placeholder="🔍 Search group folders…" value="' + esc(state.docGroupSearch) + '"/></div>' +
      '<div class="rf-folder-chip' + (state.selectedDocGroup === 'all' ? ' active' : '') + '" data-rf-dgrp="all">📂 All <span class="rf-fcnt">' + state.documents.length + '</span></div>' +
      visibleGroups.map(function (g) {
        var gs = String(g.slug || '').toLowerCase();
        var cnt = state.documents.filter(function (d) { return docGroupSlug(d) === gs; }).length;
        return '<div class="rf-folder-chip' + (state.selectedDocGroup === gs ? ' active' : '') + '" data-rf-dgrp="' + esc(gs) + '" title="' + esc(g.description || g.name) + '">' +
          '<span>📁 ' + esc(g.name) + '</span>' +
          '<span class="rf-fcnt">' + cnt + '</span>' +
          '<button type="button" class="rf-folder-act" data-rf-dgrp-menu="' + esc(gs) + '" aria-label="Folder options" title="Folder options">' + DOTS_SVG + '</button>' +
          '</div>';
      }).join('') +
      (!visibleGroups.length && gq ? '<span style="font-size:.78rem;color:rgb(var(--text-secondary))">No matching folders</span>' : '') +
      '<div class="rf-folder-chip' + (state.selectedDocGroup === 'unassigned' ? ' active' : '') + '" data-rf-dgrp="unassigned">📂 Unassigned <span class="rf-fcnt">' + unassignedCnt + '</span></div>';

    var groupOpts = function (curSlug) {
      return '<option value=""' + (!curSlug ? ' selected' : '') + '>📂 Unassigned</option>' +
        state.documentGroups.map(function (g) {
          var gs = String(g.slug || '').toLowerCase();
          return '<option value="' + esc(gs) + '"' + (curSlug === gs ? ' selected' : '') + '>📁 ' + esc(g.name) + '</option>';
        }).join('');
    };

    body.innerHTML =
      '<div class="rf-section-head"><div><h2>📁 Archival Documents &amp; Manuscripts' + (activeGroupObj ? ' — 📁 ' + esc(activeGroupObj.name) : state.selectedDocGroup === 'unassigned' ? ' — Unassigned' : '') + '</h2>' +
      '<p>' + esc(activeGroupObj && activeGroupObj.description ? activeGroupObj.description : 'Organize digitized Jawi, Pallava, Kawi, Rencong, Rumi, and English documents into maintainable group folders. Use the ⋮ menu on any folder to rename or delete it.') + '</p></div>' +
      '<div style="display:flex;gap:8px;flex-wrap:wrap">' +
      '<button type="button" class="rf-btn" id="rf-add-dgrp-btn">📁＋ New Group Folder</button>' +
      '<button type="button" class="rf-btn rf-btn-primary" id="rf-add-doc-btn">＋ Add Document</button>' +
      '</div></div>' +
      '<div class="rf-folder-bar">' + folderChips + '</div>' +
      '<div class="rf-toolbar"><input class="rf-input" id="rf-doc-search" style="flex:1;min-width:220px" type="search" placeholder="Search documents in this folder by title, code, script, or text…" value="' + esc(state.search) + '"/></div>' +
      '<div class="rf-grid">' +
      (list.length ? list.map(function (d) {
        var sub = [d.reference_code, d.script_type, d.category].filter(Boolean).join(' · ') || 'Archival Document';
        var verdict = d.authenticity_report && d.authenticity_report.verdict ? '<span class="rf-badge rf-badge-warn" style="margin-left:6px">' + esc(d.authenticity_report.verdict) + '</span>' : '';
        var curGs = docGroupSlug(d);
        return '<article class="rf-card"><div class="rf-thumb">📁</div><div class="rf-card-body">' +
          '<h3 title="' + esc(d.title) + '">' + esc(d.title) + '</h3>' +
          '<div class="rf-sub">' + esc(sub) + '</div>' +
          '<p class="rf-desc">' + esc(d.summary || d.extracted_text || 'No summary yet.') + '</p>' +
          '<div class="rf-meta">' + (d.file_count || 0) + ' file(s)' + verdict + '</div>' +
          '<div class="rf-actions">' +
          '<select class="rf-move-sel" data-rf-move-d="' + esc(d.slug) + '" title="Change group folder">' + groupOpts(curGs) + '</select>' +
          '<button type="button" class="rf-btn rf-btn-primary rf-btn-sm" data-rf-edit-d="' + esc(d.slug) + '">' + EDIT_SVG + '<span>Edit</span></button>' +
          '<button type="button" class="rf-btn rf-btn-danger rf-btn-sm" data-rf-del-d="' + esc(d.slug) + '">' + TRASH_SVG + '<span>Delete</span></button>' +
          '</div></div></article>';
      }).join('') : '<div class="rf-empty">No archival documents in this folder yet. Click "＋ Add Document" to create one.</div>') +
      '</div>';

    document.getElementById('rf-add-dgrp-btn').onclick = function () { openGroupModal('document', null); };
    document.getElementById('rf-add-doc-btn').onclick = function () { openDocumentModal(null); };
    document.getElementById('rf-doc-grp-search').oninput = function (e) {
      var pos = e.target.selectionStart;
      state.docGroupSearch = e.target.value;
      renderDocuments(body);
      restoreFocus('rf-doc-grp-search', pos);
    };
    document.getElementById('rf-doc-search').oninput = function (e) {
      var pos = e.target.selectionStart;
      state.search = e.target.value;
      renderDocuments(body);
      restoreFocus('rf-doc-search', pos);
    };

    body.querySelectorAll('[data-rf-dgrp]').forEach(function (chip) {
      chip.onclick = function (ev) {
        if (ev.target.closest('.rf-folder-act')) return;
        state.selectedDocGroup = chip.getAttribute('data-rf-dgrp');
        renderDocuments(body);
      };
    });
    body.querySelectorAll('[data-rf-dgrp-menu]').forEach(function (btn) {
      btn.onclick = function (ev) {
        ev.stopPropagation();
        var gs = btn.getAttribute('data-rf-dgrp-menu');
        var g = state.documentGroups.find(function (x) { return String(x.slug || '').toLowerCase() === gs; });
        if (!g) return;
        var memberCnt = state.documents.filter(function (d) { return docGroupSlug(d) === gs; }).length;
        openFolderMenu(btn, {
          onRename: function () { openGroupModal('document', g); },
          onDelete: function () {
            openConfirmModal({
              title: 'Delete Group Folder "' + g.name + '"?',
              message: 'This removes the "' + g.name + '" folder. The ' + memberCnt + ' document' + (memberCnt === 1 ? '' : 's') + ' inside will NOT be deleted — they will move safely to Unassigned.',
              confirmLabel: 'Delete Folder',
              onConfirm: async function () {
                var r = await api('/admin/api/document-groups/' + encodeURIComponent(g.slug), { method: 'DELETE' });
                if (r.ok) {
                  if (state.selectedDocGroup === gs) state.selectedDocGroup = 'all';
                  toast('Deleted folder "' + g.name + '"');
                  await refreshAll(false);
                } else toast((r.data && r.data.reason) || 'Delete failed', false);
              }
            });
          }
        });
      };
    });
    body.querySelectorAll('[data-rf-move-d]').forEach(function (sel) {
      sel.onchange = async function () {
        var slug = sel.getAttribute('data-rf-move-d');
        var nextGrp = sel.value;
        var r = await api('/admin/api/documents/' + encodeURIComponent(slug), {
          method: 'PATCH',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ group_slug: nextGrp })
        });
        if (r.ok) {
          toast(nextGrp ? 'Moved to folder' : 'Moved to Unassigned');
          await refreshAll(false);
        } else {
          toast((r.data && r.data.reason) || 'Move failed', false);
          renderDocuments(body);
        }
      };
    });
    body.querySelectorAll('[data-rf-edit-d]').forEach(function (b) {
      b.onclick = function () {
        var d = state.documents.find(function (x) { return x.slug === b.getAttribute('data-rf-edit-d'); });
        openDocumentModal(d || null);
      };
    });
    body.querySelectorAll('[data-rf-del-d]').forEach(function (b) {
      b.onclick = function () {
        var d = state.documents.find(function (x) { return x.slug === b.getAttribute('data-rf-del-d'); });
        if (!d) return;
        openConfirmModal({
          title: 'Delete Document "' + d.title + '"?',
          message: 'Delete document "' + d.title + '" and its ' + (d.file_count || 0) + ' uploaded file(s)? This cannot be undone.',
          confirmLabel: 'Delete Document',
          onConfirm: async function () {
            var r = await api('/admin/api/documents/' + encodeURIComponent(d.slug), { method: 'DELETE' });
            if (r.ok) { toast('Deleted document'); await refreshAll(false); }
            else toast((r.data && r.data.reason) || 'Delete failed', false);
          }
        });
      };
    });
  }

  function openDocumentModal(doc) {
    var wrap = document.createElement('div');
    wrap.className = 'rf-modal-backdrop';
    var scripts = ['Jawi', 'Pallava', 'Kawi', 'Rencong', 'Rumi / Bahasa Melayu', 'English', 'Multilingual'];
    var defaultGrp = doc ? docGroupSlug(doc) : (state.selectedDocGroup !== 'all' && state.selectedDocGroup !== 'unassigned' ? state.selectedDocGroup : '');
    var grpSelectHtml =
      '<option value=""' + (!defaultGrp ? ' selected' : '') + '>Unassigned</option>' +
      state.documentGroups.map(function (g) {
        var gs = String(g.slug || '').toLowerCase();
        return '<option value="' + esc(gs) + '"' + (defaultGrp === gs ? ' selected' : '') + '>' + esc(g.name) + '</option>';
      }).join('');
    wrap.innerHTML =
      '<div class="rf-modal"><h2>' + (doc ? 'Edit Document' : 'Add Archival Document') + '</h2>' +
      '<form id="rf-d-form"><div class="rf-form-grid">' +
      '<div class="rf-field"><label>Document Title</label><input class="rf-input" id="rf-d-title" required value="' + esc(doc ? doc.title || '' : '') + '"/></div>' +
      '<div class="rf-field"><label>Group Folder</label><select class="rf-select" id="rf-d-group">' + grpSelectHtml + '</select></div>' +
      '<div class="rf-field"><label>Reference Code</label><input class="rf-input" id="rf-d-ref" value="' + esc(doc ? doc.reference_code || '' : '') + '"/></div>' +
      '<div class="rf-field"><label>Category</label><input class="rf-input" id="rf-d-cat" value="' + esc(doc ? doc.category || '' : '') + '"/></div>' +
      '<div class="rf-field"><label>Script / Writing System</label><select class="rf-select" id="rf-d-script">' +
      scripts.map(function (s) { return '<option value="' + esc(s) + '"' + (doc && doc.script_type === s ? ' selected' : '') + '>' + esc(s) + '</option>'; }).join('') +
      '</select></div>' +
      '<div class="rf-field"><label>Date / Era</label><input class="rf-input" id="rf-d-date" value="' + esc(doc ? doc.date_created || '' : '') + '"/></div>' +
      '<div class="rf-field"><label>Origin / Location</label><input class="rf-input" id="rf-d-orig" value="' + esc(doc ? doc.origin_location || '' : '') + '"/></div>' +
      '<div class="rf-field"><label>Author / Source</label><input class="rf-input" id="rf-d-auth" value="' + esc(doc ? doc.author_or_source || '' : '') + '"/></div>' +
      (doc ? '<div class="rf-field full"><label>Uploaded Files (TXT, MD, CSV, JSON, HTML, PDF, DOCX, JPG, PNG · duplicate &amp; authenticity checked)</label><div id="rf-d-files" style="display:flex;flex-wrap:wrap;gap:8px;margin-bottom:6px"></div><input class="rf-input" id="rf-d-addfile" type="file" accept=".txt,.md,.csv,.json,.html,.htm,.xml,.pdf,.docx,.jpg,.jpeg,.png,.webp,.bmp"/><div id="rf-d-authinfo" style="font-size:.8rem;margin-top:6px"></div></div>' : '') +
      '<div class="rf-field full"><div style="display:flex;justify-content:space-between;align-items:center"><label>Summary</label>' + (doc ? '<button type="button" class="rf-btn rf-btn-ok rf-btn-sm" id="rf-d-sum-btn">✨ Auto-Summarize</button>' : '') + '</div><textarea class="rf-textarea" id="rf-d-sum">' + esc(doc ? doc.summary || '' : '') + '</textarea></div>' +
      '<div class="rf-field full"><label>Digitized / Extracted Text</label><textarea class="rf-textarea" id="rf-d-text" style="min-height:110px">' + esc(doc ? doc.extracted_text || '' : '') + '</textarea></div>' +
      '<div class="rf-field"><label>Key Topics (comma-separated)</label><input class="rf-input" id="rf-d-topics" value="' + esc(doc && Array.isArray(doc.key_topics) ? doc.key_topics.join(', ') : '') + '"/></div>' +
      '<div class="rf-field"><label>Key Names (comma-separated)</label><input class="rf-input" id="rf-d-names" value="' + esc(doc && Array.isArray(doc.key_names) ? doc.key_names.join(', ') : '') + '"/></div>' +
      '<div class="rf-field full"><label>Key Dates (comma-separated)</label><input class="rf-input" id="rf-d-dates" value="' + esc(doc && Array.isArray(doc.key_dates) ? doc.key_dates.join(', ') : '') + '"/></div>' +
      '<div class="rf-field full"><label>Notes</label><textarea class="rf-textarea" id="rf-d-notes">' + esc(doc ? doc.notes || '' : '') + '</textarea></div>' +
      '</div><div class="rf-actions" style="justify-content:flex-end;margin-top:16px">' +
      '<button type="button" class="rf-btn" id="rf-d-cancel">Cancel</button>' +
      '<button type="submit" class="rf-btn rf-btn-primary">Save Document</button></div></form></div>';

    document.body.appendChild(wrap);
    wrap.onclick = function (e) { if (e.target === wrap) wrap.remove(); };
    document.getElementById('rf-d-cancel').onclick = function () { wrap.remove(); };

    function renderDocFiles(cur) {
      var box = document.getElementById('rf-d-files');
      if (!box) return;
      var files = cur.files || [];
      box.innerHTML = files.length ? files.map(function (f) {
        return '<div class="rf-btn rf-btn-sm" style="gap:8px"><a href="' + esc(apiUrl(f.admin_url)) + '" target="_blank" style="color:inherit;text-decoration:none">📄 ' + esc(f.filename) + '</a><button type="button" class="rf-icon-del" data-del-f="' + esc(f.filename) + '" title="Delete file" style="width:22px;height:22px">' + TRASH_SVG + '</button></div>';
      }).join('') : '<span style="font-size:.78rem;color:rgb(var(--text-secondary))">No files uploaded yet.</span>';
      box.querySelectorAll('[data-del-f]').forEach(function (x) {
        x.onclick = function () {
          var fn = x.getAttribute('data-del-f');
          openConfirmModal({
            title: 'Delete File?',
            message: 'Delete file "' + fn + '" from document "' + cur.title + '"? This cannot be undone.',
            confirmLabel: 'Delete File',
            onConfirm: async function () {
              var r = await api('/admin/api/documents/' + encodeURIComponent(cur.slug) + '/files/' + encodeURIComponent(fn), { method: 'DELETE' });
              if (r.ok && r.data.document) { cur = r.data.document; renderDocFiles(cur); toast('File deleted'); refreshAll(false); }
            }
          });
        };
      });
      var ai = document.getElementById('rf-d-authinfo');
      if (ai) {
        ai.innerHTML = cur.authenticity_report && cur.authenticity_report.verdict
          ? '<b>🛡️ Authenticity:</b> ' + esc(cur.authenticity_report.plain_language_verdict || cur.authenticity_report.verdict)
          : '';
      }
    }

    if (doc) {
      renderDocFiles(doc);
      var fIn = document.getElementById('rf-d-addfile');
      async function doDocUpload(file, force) {
        var fd = new FormData();
        fd.append('file', file);
        var r = await api('/admin/api/documents/' + encodeURIComponent(doc.slug) + '/files' + (force ? '?force=1' : ''), { method: 'POST', body: fd });
        if (r.status === 409 && !force) {
          openConfirmModal({
            title: 'Duplicate File Detected',
            message: (r.data.reason || 'This file already exists in the archive.') + ' Do you still want to upload a duplicate copy?',
            confirmLabel: 'Upload Anyway',
            primary: true,
            onConfirm: function () { return doDocUpload(file, true); },
            onCancel: function () { toast('Duplicate upload prevented', false); }
          });
          return;
        }
        if (r.ok && r.data.document) {
          doc = r.data.document;
          document.getElementById('rf-d-text').value = doc.extracted_text || '';
          document.getElementById('rf-d-sum').value = doc.summary || '';
          renderDocFiles(doc);
          toast('File uploaded & verified');
          refreshAll(false);
        } else toast((r.data && r.data.reason) || 'Upload failed', false);
      }
      fIn.onchange = function () { if (fIn.files[0]) { doDocUpload(fIn.files[0], false); fIn.value = ''; } };

      document.getElementById('rf-d-sum-btn').onclick = async function (ev) {
        var btn = ev.target;
        btn.disabled = true; btn.textContent = '⏳ Summarizing…';
        var r = await api('/admin/api/documents/' + encodeURIComponent(doc.slug) + '/summarize', { method: 'POST' });
        btn.disabled = false; btn.textContent = '✨ Auto-Summarize';
        if (r.ok && r.data.document) {
          doc = r.data.document;
          document.getElementById('rf-d-sum').value = doc.summary || '';
          document.getElementById('rf-d-topics').value = (doc.key_topics || []).join(', ');
          document.getElementById('rf-d-names').value = (doc.key_names || []).join(', ');
          document.getElementById('rf-d-dates').value = (doc.key_dates || []).join(', ');
          toast('Summary generated');
          refreshAll(false);
        } else toast((r.data && r.data.reason) || 'Summarization failed', false);
      };
    }

    document.getElementById('rf-d-form').onsubmit = async function (ev) {
      ev.preventDefault();
      var payload = {
        title: document.getElementById('rf-d-title').value,
        group_slug: document.getElementById('rf-d-group').value,
        reference_code: document.getElementById('rf-d-ref').value,
        category: document.getElementById('rf-d-cat').value,
        script_type: document.getElementById('rf-d-script').value,
        date_created: document.getElementById('rf-d-date').value,
        origin_location: document.getElementById('rf-d-orig').value,
        author_or_source: document.getElementById('rf-d-auth').value,
        summary: document.getElementById('rf-d-sum').value,
        extracted_text: document.getElementById('rf-d-text').value,
        key_topics: document.getElementById('rf-d-topics').value,
        key_names: document.getElementById('rf-d-names').value,
        key_dates: document.getElementById('rf-d-dates').value,
        notes: document.getElementById('rf-d-notes').value
      };
      var r = await api(doc ? '/admin/api/documents/' + encodeURIComponent(doc.slug) : '/admin/api/documents', {
        method: doc ? 'PATCH' : 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload)
      });
      if (r.ok) { wrap.remove(); toast(doc ? 'Document updated' : 'Document folder created'); await refreshAll(false); }
      else toast((r.data && r.data.reason) || 'Save failed', false);
    };
  }

  // ─── Tab 5: System Monitor, File Integrity & Conservation Lab ─────────────
  function fmtUp(s) {
    var h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), sec = s % 60;
    return h > 0 ? h + 'h ' + m + 'm' : m > 0 ? m + 'm ' + sec + 's' : sec + 's';
  }
  function dmgBadge(lvl) {
    var cls = lvl === 'heavy' ? 'rf-badge-crit' : (lvl === 'medium' || lvl === 'light') ? 'rf-badge-warn' : 'rf-badge-ok';
    return '<span class="rf-badge ' + cls + '">' + esc(lvl || 'none') + '</span>';
  }

  function renderSystem(body) {
    var d = state.health || {};
    var res = d.resources || {}, svc = d.services || {}, wl = d.workload || {};
    var st = d.status || 'CHECKING';
    var badgeCls = st === 'CRITICAL_OVERLOAD' ? 'rf-badge-crit' : st === 'WARNING' ? 'rf-badge-warn' : 'rf-badge-ok';
    var abn = d.abnormalities || [];
    var acts = d.recommended_actions || [];

    var intgHtml = '';
    if (state.integrity) {
      var j = state.integrity;
      var ok = j.integrity_status === 'VERIFIED_INTACT';
      intgHtml = '<div class="rf-box"><h3>🛡️ File Integrity Check — <span class="rf-badge ' + (ok ? 'rf-badge-ok' : 'rf-badge-crit') + '">' + esc(j.integrity_status) + '</span></h3>' +
        '<p>Scanned: <b>' + (j.total_files_scanned || 0) + '</b> · Verified intact: <b>' + (j.verified_intact_count || 0) + '</b> · Newly indexed: <b>' + (j.newly_indexed_count || 0) + '</b> · Corrupted: <b>' + (j.corrupted_count || 0) + '</b> · Missing: <b>' + (j.missing_count || 0) + '</b> · Duplicate groups: <b>' + (j.duplicate_groups_count || 0) + '</b></p></div>';
    }

    body.innerHTML =
      '<div class="rf-section-head"><div><h2>📊 System Health, File Integrity &amp; Conservation Lab</h2><p>Monitor live server performance, detect overload/abnormalities, verify archive checksums, and analyze or restore archival scans.</p></div>' +
      '<span class="rf-badge ' + badgeCls + '" style="font-size:.78rem;padding:5px 12px">' + esc(st) + '</span></div>' +
      '<div class="rf-stats">' +
      '<div class="rf-stat"><strong>' + (res.cpu_usage_pct != null ? res.cpu_usage_pct + '%' : '—') + '</strong><span>CPU Usage</span></div>' +
      '<div class="rf-stat"><strong>' + (res.ram_usage_pct != null ? res.ram_used_gb + ' / ' + res.ram_total_gb + ' GB (' + res.ram_usage_pct + '%)' : '—') + '</strong><span>System Memory</span></div>' +
      '<div class="rf-stat"><strong>' + (res.disk_used_pct != null ? res.disk_used_pct + '% (' + res.disk_available_gb + ' GB free)' : '—') + '</strong><span>Storage Disk</span></div>' +
      '<div class="rf-stat"><strong>' + (res.server_process_ram_mb != null ? res.server_process_ram_mb + ' MB · ' + fmtUp(d.uptime_seconds || 0) : '—') + '</strong><span>Server RAM · Uptime</span></div>' +
      '<div class="rf-stat"><strong>' + (svc.ollama_reachable ? 'Online (' + svc.ollama_ping_ms + ' ms)' : 'Offline') + '</strong><span>Local Vision AI</span></div>' +
      '<div class="rf-stat"><strong>' + (wl.total_requests || 0) + ' req · ' + (wl.avg_latency_ms || 0) + ' ms</strong><span>Workload (' + (wl.in_flight_requests || 0) + ' active)</span></div>' +
      '</div>' +
      '<div class="rf-box"><h3>🧭 Abnormalities &amp; Recommended Actions</h3>' +
      (abn.length ? '<ul>' + abn.map(function (a) { return '<li><b>[' + esc(a.severity) + '] ' + esc(a.component) + ':</b> ' + esc(a.detail) + '</li>'; }).join('') + '</ul>' : '<div style="color:#059669;font-weight:600">✅ No abnormalities or overload detected.</div>') +
      '<ul style="margin-top:8px">' + acts.map(function (x) { return '<li>' + esc(x) + '</li>'; }).join('') + '</ul></div>' +
      '<div class="rf-actions" style="margin-top:14px">' +
      '<button type="button" class="rf-btn" id="rf-sys-ref">↻ Refresh Telemetry</button>' +
      '<button type="button" class="rf-btn rf-btn-primary" id="rf-sys-intg">🛡️ Verify Archive Integrity</button>' +
      '<button type="button" class="rf-btn rf-btn-ok" id="rf-sys-clr">🧹 Clear Temp Image Cache</button>' +
      '</div>' + intgHtml +
      '<div class="rf-box" style="margin-top:18px"><h3>🧪 Archival Image Quality, Damage &amp; Enhancement Lab</h3>' +
      '<p style="color:rgb(var(--text-secondary));margin-bottom:10px">Upload a photo or manuscript scan to check quality, detect tears/fading/mold/aging (light/medium/heavy), check authenticity, get physical healing suggestions, or enhance image clarity.</p>' +
      '<div class="rf-toolbar" style="margin-bottom:10px">' +
      '<input class="rf-input" id="rf-lab-file" type="file" accept=".jpg,.jpeg,.png,.webp,.bmp"/>' +
      '<select class="rf-select" id="rf-lab-mode">' +
      '<option value="auto">Mode: Auto Archival Restore</option>' +
      '<option value="manuscript_ocr">Mode: Manuscript OCR Boost</option>' +
      '<option value="deyellow_restore">Mode: De-Yellow &amp; Paper Restore</option>' +
      '<option value="contrast_lighting">Mode: Lighting &amp; Contrast Fix</option>' +
      '<option value="denoise_sharpen">Mode: Crisp Sharpen</option>' +
      '</select>' +
      '<button type="button" class="rf-btn rf-btn-primary" id="rf-lab-ana">🔬 Analyze Quality &amp; Damage</button>' +
      '<button type="button" class="rf-btn rf-btn-ok" id="rf-lab-enh">✨ Enhance Image</button>' +
      '</div><div id="rf-lab-out">' + state.labHtml + '</div></div>';

    document.getElementById('rf-sys-ref').onclick = async function () {
      var r = await api('/admin/api/system/health?ts=' + Date.now());
      if (r.ok) { state.health = r.data; renderSystem(body); toast('Telemetry refreshed'); }
    };
    document.getElementById('rf-sys-intg').onclick = async function () {
      var r = await api('/admin/api/system/integrity', { method: 'POST' });
      if (r.ok) { state.integrity = r.data; renderSystem(body); toast('Integrity check complete'); }
    };
    document.getElementById('rf-sys-clr').onclick = async function () {
      var r = await api('/admin/api/system/clear-cache', { method: 'POST' });
      if (r.ok) { toast('Cleared ' + (r.data.removed_files || 0) + ' temp file(s)'); refreshAll(false); }
    };
    document.getElementById('rf-lab-ana').onclick = async function () {
      var f = document.getElementById('rf-lab-file').files[0];
      if (!f) { toast('Choose an image first', false); return; }
      var fd = new FormData(); fd.append('image', f);
      document.getElementById('rf-lab-out').innerHTML = '⏳ Analyzing quality, damage &amp; authenticity…';
      var r = await api('/admin/api/conservation/analyze', { method: 'POST', body: fd });
      if (!r.ok) { toast(r.data.reason || 'Analysis failed', false); return; }
      var j = r.data, q = j.image_quality || {}, dm = j.damage_assessment || {}, au = j.authenticity_summary || {};
      var sugg = (j.conservation_healing_suggestions || []).map(function (s) {
        return '<li><b>' + esc(s.treatment) + '</b> (' + dmgBadge(s.severity) + '): ' + (s.actions || []).map(esc).join(' ') + '</li>';
      }).join('');
      var riskCls = au.fraud_risk === 'HIGH' ? 'rf-badge-crit' : au.fraud_risk === 'MEDIUM' ? 'rf-badge-warn' : 'rf-badge-ok';
      var wp = au.watermark_and_provenance || {}, js = au.jpeg_structure || {}, px = au.pixel_forensics || {}, va = au.visual_forensic_audit || {};
      var sigHtml = (au.forensic_signals || []).length
        ? '<ul style="margin:6px 0 0 18px">' + au.forensic_signals.map(function (s) { return '<li>' + esc(s) + '</li>'; }).join('') + '</ul>'
        : '';
      var authHtml = au.verdict
        ? '<div class="rf-box" style="margin-top:4px;padding:12px">' +
          '<div><b>🛡️ Deep Authenticity &amp; Watermark Check:</b> <span class="rf-badge ' + riskCls + '">' + esc(au.fraud_risk) + ' RISK</span> <b>' + esc(au.verdict) + '</b></div>' +
          (au.plain_language_verdict ? '<div style="margin-top:4px">' + esc(au.plain_language_verdict) + '</div>' : '') +
          '<div style="font-size:.76rem;color:rgb(var(--text-secondary));margin-top:6px">' +
          'Camera EXIF: <b>' + (au.camera_exif_present ? 'Present' : 'Missing') + '</b> · ' +
          'C2PA: <b>' + (au.c2pa_present ? 'Present' : 'None') + '</b> · ' +
          'Visible Watermark: <b>' + esc(wp.visible_watermark_detected || (va.visible_watermark || 'none')) + '</b>' +
          (js.is_bare_stripped_jpeg ? ' · <b style="color:#f59e0b">Bare Stripped JPEG (0 APP/EXIF headers)</b>' : '') +
          (px.ela_p95_to_median_ratio != null ? ' · ELA ratio: <b>' + px.ela_p95_to_median_ratio + '</b> · Noise CV: <b>' + px.noise_variation_cv + '</b> · 8×8 Blockiness: <b>' + px.jpeg_blockiness_ratio + '</b>' : '') +
          '</div>' + sigHtml + '</div>'
        : '';
      state.labHtml =
        '<div style="display:grid;gap:8px;margin-top:8px">' +
        '<div><b>📐 Image Quality:</b> <b>' + (q.overall_score || 0) + '/100 (' + esc(q.overall_grade) + ')</b> · Resolution: ' + (q.resolution ? q.resolution.width + '×' + q.resolution.height : '') + ' · Lighting: ' + (q.lighting ? q.lighting.score + '/100 (' + esc(q.lighting.condition) + ')' : '') + ' · Readability: ' + (q.readability ? q.readability.score + '/100' : '') + '</div>' +
        '<div><b>🩹 Damage (Overall ' + dmgBadge(dm.overall_severity) + '):</b> Tear ' + dmgBadge(dm.tear && dm.tear.level) + ' · Faded ' + dmgBadge(dm.faded && dm.faded.level) + ' · Mold ' + dmgBadge(dm.mold && dm.mold.level) + ' · Aging ' + dmgBadge(dm.aging && dm.aging.level) + '</div>' +
        authHtml +
        '<div><b>💊 Conservation &amp; Healing Recommendations:</b><ul>' + sugg + '</ul></div></div>';
      document.getElementById('rf-lab-out').innerHTML = state.labHtml;
    };
    document.getElementById('rf-lab-enh').onclick = async function () {
      var f = document.getElementById('rf-lab-file').files[0];
      if (!f) { toast('Choose an image first', false); return; }
      var fd = new FormData();
      fd.append('image', f);
      fd.append('mode', document.getElementById('rf-lab-mode').value);
      document.getElementById('rf-lab-out').innerHTML = '✨ Enhancing image clarity…';
      var r = await api('/admin/api/conservation/enhance', { method: 'POST', body: fd });
      if (!r.ok) { toast(r.data.reason || 'Enhancement failed', false); return; }
      var j = r.data, bq = j.before_quality || {}, aq = j.after_quality || {};
      var imgSrc = j.admin_preview_url ? apiUrl(j.admin_preview_url) : '';
      state.labHtml =
        '<div style="margin-top:8px"><b>✨ Enhanced (' + esc(j.mode) + '):</b> Quality <b>' + bq.overall_score + ' → ' + aq.overall_score + '/100</b> · Readability ' + (bq.readability && bq.readability.score) + ' → ' + (aq.readability && aq.readability.score) + '</div>' +
        (imgSrc ? '<img src="' + esc(imgSrc) + '" id="rf-enh-prev" style="margin-top:10px;max-width:440px;border-radius:10px;border:1px solid rgb(var(--border-medium));cursor:pointer;display:block"/>' : '');
      document.getElementById('rf-lab-out').innerHTML = state.labHtml;
      var pv = document.getElementById('rf-enh-prev');
      if (pv) pv.onclick = function () { openViewer(imgSrc); };
      toast('Image enhanced');
    };
  }

  // ─── Tab 6: Test Face Recognition (Upload, Drag & Drop, or Clipboard Paste) ─
  function guessExtFromMime(mime) {
    var m = String(mime || '').toLowerCase();
    if (m.indexOf('jpeg') !== -1 || m.indexOf('jpg') !== -1) return 'jpg';
    if (m.indexOf('webp') !== -1) return 'webp';
    if (m.indexOf('bmp') !== -1) return 'bmp';
    return 'png';
  }

  function setTestFaceFile(blobOrFile, label) {
    if (!blobOrFile) return;
    if (state.testPreview) {
      try { URL.revokeObjectURL(state.testPreview); } catch (e) {}
    }
    var fname = (blobOrFile.name && /\.(jpe?g|png|webp|bmp)$/i.test(blobOrFile.name))
      ? blobOrFile.name
      : ((label || 'pasted-image') + '.' + guessExtFromMime(blobOrFile.type));
    state.testFile = blobOrFile;
    state.testFileName = fname;
    state.testPreview = URL.createObjectURL(blobOrFile);
    state.testResultHtml = '';
    state.testUnrecognized = false;
    state.testEnrollSlug = '';
    if (state.open && state.authed && state.tab === 'test') {
      var body = document.getElementById('rf-main-body');
      if (body) renderTestFace(body);
    }
  }

  function clearTestFaceFile() {
    if (state.testPreview) {
      try { URL.revokeObjectURL(state.testPreview); } catch (e) {}
    }
    state.testFile = null;
    state.testFileName = '';
    state.testPreview = '';
    state.testResultHtml = '';
    state.testUnrecognized = false;
    state.testEnrollSlug = '';
    var body = document.getElementById('rf-main-body');
    if (body && state.tab === 'test') renderTestFace(body);
  }

  function renderTestEnrollPrompt(container, body) {
    if (!container) return;
    if (!state.testUnrecognized || !state.testFile) {
      container.innerHTML = '';
      return;
    }
    var fq = (state.testEnrollFilter || '').trim().toLowerCase();
    var sortedProfiles = state.profiles.slice().sort(function (a, b) {
      return String(a.identity || '').localeCompare(String(b.identity || ''));
    });
    var filteredProfiles = sortedProfiles.filter(function (p) {
      return !fq || (String(p.identity || '') + ' ' + String(p.group || '')).toLowerCase().indexOf(fq) !== -1;
    });
    var selectedProfile = state.profiles.find(function (p) { return p.slug === state.testEnrollSlug; });
    var grpOpts = '<option value=""' + (!state.testEnrollNewGroup ? ' selected' : '') + '>📂 Unassigned</option>' +
      state.peopleGroups.map(function (g) {
        var gs = String(g.slug || '').toLowerCase();
        return '<option value="' + esc(gs) + '"' + (state.testEnrollNewGroup === gs ? ' selected' : '') + '>📁 ' + esc(g.name) + '</option>';
      }).join('');

    var existingHtml =
      '<div style="display:flex;gap:8px;align-items:center;flex-wrap:wrap">' +
      '<input class="rf-input" id="rf-te-filter" type="search" style="flex:1;min-width:200px" placeholder="🔍 Quick filter existing people (or just click a name below)…" value="' + esc(state.testEnrollFilter) + '"/>' +
      '</div>' +
      '<div class="rf-pick-list">' +
      (filteredProfiles.length ? filteredProfiles.map(function (p) {
        var av = p.images && p.images[0]
          ? '<img class="rf-pick-av" src="' + esc(apiUrl(p.images[0].url)) + '" alt="' + esc(p.identity) + '"/>'
          : '<div class="rf-pick-av">👤</div>';
        var sub = (p.group ? p.group + ' · ' : '') + Number(p.vector_count || 0) + ' vector(s)';
        return '<button type="button" class="rf-pick-item' + (state.testEnrollSlug === p.slug ? ' selected' : '') + '" data-rf-pick="' + esc(p.slug) + '">' +
          av + '<div class="rf-pick-info"><div class="rf-pick-name" title="' + esc(p.identity) + '">' + esc(p.identity) + '</div><div class="rf-pick-meta">' + esc(sub) + '</div></div></button>';
      }).join('') : '<div style="font-size:.8rem;color:rgb(var(--text-secondary));padding:8px">No enrolled people match "' + esc(state.testEnrollFilter) + '". Switch to "＋ New Person" to enroll them.</div>') +
      '</div>' +
      '<div class="rf-actions" style="margin-top:12px">' +
      '<button type="button" class="rf-btn rf-btn-ok" id="rf-te-save-existing"' + (!selectedProfile ? ' disabled' : '') + '>💾 ' + (selectedProfile ? 'Save Image &amp; Vector to "' + esc(selectedProfile.identity) + '"' : 'Select a Person Above to Enroll') + '</button>' +
      '</div>';

    var newHtml =
      '<div class="rf-form-grid">' +
      '<div class="rf-field"><label>Full Name</label><input class="rf-input" id="rf-te-new-name" placeholder="e.g. Ahmad Faizal" value="' + esc(state.testEnrollNewName) + '"/></div>' +
      '<div class="rf-field"><label>Group Folder</label><select class="rf-select" id="rf-te-new-grp">' + grpOpts + '</select></div>' +
      '</div>' +
      '<div class="rf-actions" style="margin-top:12px">' +
      '<button type="button" class="rf-btn rf-btn-ok" id="rf-te-save-new">＋ Enroll New Person &amp; Save Vector</button>' +
      '</div>';

    container.innerHTML =
      '<div class="rf-enroll-box">' +
      '<h3 style="margin:0 0 4px;font-size:.96rem">👤 Face Not Recognized — Declare Who Is In This Image?</h3>' +
      '<p style="margin:0 0 12px;font-size:.82rem;color:rgb(var(--text-secondary))">Pick an existing enrolled person below (no need to type letter by letter) or create a new person. Proceeding saves both the photo and its 768-D face vector into the vector database.</p>' +
      '<div class="rf-mode-tabs">' +
      '<button type="button" class="rf-mode-tab' + (state.testEnrollMode === 'existing' ? ' active' : '') + '" data-rf-te-mode="existing">👥 Existing Person (' + state.profiles.length + ')</button>' +
      '<button type="button" class="rf-mode-tab' + (state.testEnrollMode === 'new' ? ' active' : '') + '" data-rf-te-mode="new">＋ New Person</button>' +
      '</div>' +
      (state.testEnrollMode === 'existing' ? existingHtml : newHtml) +
      '</div>';

    container.querySelectorAll('[data-rf-te-mode]').forEach(function (btn) {
      btn.onclick = function () {
        state.testEnrollMode = btn.getAttribute('data-rf-te-mode');
        renderTestEnrollPrompt(container, body);
      };
    });

    if (state.testEnrollMode === 'existing') {
      var filterEl = document.getElementById('rf-te-filter');
      if (filterEl) {
        filterEl.oninput = function (e) {
          var pos = e.target.selectionStart;
          state.testEnrollFilter = e.target.value;
          renderTestEnrollPrompt(container, body);
          restoreFocus('rf-te-filter', pos);
        };
      }
      container.querySelectorAll('[data-rf-pick]').forEach(function (item) {
        item.onclick = function () {
          state.testEnrollSlug = item.getAttribute('data-rf-pick');
          renderTestEnrollPrompt(container, body);
        };
      });
      var saveEx = document.getElementById('rf-te-save-existing');
      if (saveEx) {
        saveEx.onclick = async function () {
          var target = state.profiles.find(function (p) { return p.slug === state.testEnrollSlug; });
          if (!target || !state.testFile) return;
          saveEx.disabled = true;
          saveEx.textContent = '⏳ Saving photo & vector…';
          try {
            var fd = new FormData();
            fd.append('image', state.testFile, state.testFileName || ('enrolled.' + guessExtFromMime(state.testFile.type)));
            var r = await api('/admin/api/profiles/' + encodeURIComponent(target.slug) + '/images', { method: 'POST', body: fd });
            if (!r.ok) {
              toast((r.data && r.data.reason) || 'Could not enroll image', false);
              saveEx.disabled = false;
              renderTestEnrollPrompt(container, body);
              return;
            }
            var prof = r.data && r.data.profile;
            var vc = prof ? Number(prof.vector_count || 0) : Number(r.data.vector_count || 0);
            var pc = prof && prof.images ? prof.images.length : 0;
            state.testUnrecognized = false;
            state.testResultHtml =
              '<div class="rf-box" style="margin-top:0;border-color:#059669">' +
              '<div style="color:#059669;font-weight:600;margin-bottom:6px">✅ Enrolled into Vector Database!</div>' +
              '<div>Saved image &amp; 768-D face vector for <b>' + esc(target.identity) + '</b> (' + vc + ' vector(s) · ' + pc + ' photo(s) now stored).</div>' +
              '<div class="rf-actions" style="margin-top:10px"><button type="button" class="rf-btn rf-btn-primary rf-btn-sm" id="rf-te-retest">🔄 Re-Test Recognition Now</button></div>' +
              '</div>';
            toast('Saved image & vector for ' + target.identity);
            await refreshAll(false);
            var b = document.getElementById('rf-main-body');
            if (b && state.tab === 'test') renderTestFace(b);
          } catch (err) {
            toast('Error: ' + err.message, false);
            saveEx.disabled = false;
          }
        };
      }
    } else {
      var nameIn = document.getElementById('rf-te-new-name');
      var grpIn = document.getElementById('rf-te-new-grp');
      if (nameIn) nameIn.oninput = function (e) { state.testEnrollNewName = e.target.value; };
      if (grpIn) grpIn.onchange = function (e) { state.testEnrollNewGroup = e.target.value; };
      var saveNew = document.getElementById('rf-te-save-new');
      if (saveNew) {
        saveNew.onclick = async function () {
          var newName = (state.testEnrollNewName || '').trim();
          if (!newName) { toast('Enter a name for the new person', false); return; }
          if (!state.testFile) return;
          saveNew.disabled = true;
          saveNew.textContent = '⏳ Enrolling person & vector…';
          try {
            var fd = new FormData();
            fd.append('identity', newName);
            fd.append('group_slug', state.testEnrollNewGroup || '');
            fd.append('image', state.testFile, state.testFileName || ('enrolled.' + guessExtFromMime(state.testFile.type)));
            var r = await api('/admin/api/profiles', { method: 'POST', body: fd });
            if (!r.ok) {
              toast((r.data && r.data.reason) || 'Enrollment failed', false);
              saveNew.disabled = false;
              renderTestEnrollPrompt(container, body);
              return;
            }
            state.testUnrecognized = false;
            state.testEnrollNewName = '';
            state.testResultHtml =
              '<div class="rf-box" style="margin-top:0;border-color:#059669">' +
              '<div style="color:#059669;font-weight:600;margin-bottom:6px">✅ New Person Enrolled into Vector Database!</div>' +
              '<div>Saved image &amp; 768-D face vector for <b>' + esc(newName) + '</b>.</div>' +
              '<div class="rf-actions" style="margin-top:10px"><button type="button" class="rf-btn rf-btn-primary rf-btn-sm" id="rf-te-retest">🔄 Re-Test Recognition Now</button></div>' +
              '</div>';
            toast('Enrolled ' + newName);
            await refreshAll(false);
            var b = document.getElementById('rf-main-body');
            if (b && state.tab === 'test') renderTestFace(b);
          } catch (err) {
            toast('Error: ' + err.message, false);
            saveNew.disabled = false;
          }
        };
      }
    }
  }

  function renderTestFace(body) {
    var sizeKb = state.testFile && state.testFile.size ? ' (' + Math.max(1, Math.round(state.testFile.size / 1024)) + ' KB)' : '';
    body.innerHTML =
      '<div class="rf-section-head"><div><h2>🔍 Test Face Recognition</h2><p>Upload, drag &amp; drop, or paste a copied image (Ctrl+V) to test face matching and confidence scores. Unrecognized faces can be enrolled directly into the vector database.</p></div></div>' +
      '<div class="rf-box" style="margin-top:0">' +
      '<div class="rf-dropzone" id="rf-tf-drop">' +
      '<div class="rf-toolbar" style="margin-bottom:0">' +
      '<label class="rf-btn" style="cursor:pointer" for="rf-tf-file">📂 Choose Photo</label>' +
      '<input id="rf-tf-file" type="file" accept=".jpg,.jpeg,.png,.webp,.bmp" style="display:none"/>' +
      '<button type="button" class="rf-btn" id="rf-tf-paste">📋 Paste from Clipboard</button>' +
      '<label style="display:inline-flex;align-items:center;gap:6px;font-size:.82rem;color:rgb(var(--text-secondary))">Min confidence:<input class="rf-input" id="rf-tf-thr" type="number" min="50" max="99" step="1" value="' + esc(state.testThresholdPct || '88') + '" style="width:74px;padding:7px 9px" title="Minimum confidence (%)"/>%</label>' +
      '<button type="button" class="rf-btn rf-btn-primary" id="rf-tf-run">🔍 Test Recognition</button>' +
      (state.testFile ? '<button type="button" class="rf-btn rf-btn-danger rf-btn-sm" id="rf-tf-clear">' + TRASH_SVG + '<span>Clear</span></button>' : '') +
      '</div>' +
      (state.testFile
        ? '<div style="margin-top:10px"><span class="rf-file-pill">🖼️ ' + esc(state.testFileName) + esc(sizeKb) + '</span></div>'
        : '<div style="margin-top:10px;font-size:.8rem;color:rgb(var(--text-secondary))">Tip: Copy any image (screenshot or web photo) and press <b>Ctrl+V</b> anywhere here, click <b>Paste from Clipboard</b>, or drop an image file into this box.</div>') +
      (state.testPreview ? '<img src="' + esc(state.testPreview) + '" id="rf-tf-img" style="margin-top:12px;max-height:320px;border-radius:10px;border:1px solid rgb(var(--border-medium));display:block;cursor:pointer"/>' : '') +
      '</div>' +
      '<div id="rf-tf-out" style="margin-top:14px">' + state.testResultHtml + '</div>' +
      '<div id="rf-tf-enroll"></div></div>';

    var enrollContainer = document.getElementById('rf-tf-enroll');
    renderTestEnrollPrompt(enrollContainer, body);

    var fileInput = document.getElementById('rf-tf-file');
    fileInput.onchange = function (e) {
      if (e.target.files && e.target.files[0]) {
        setTestFaceFile(e.target.files[0]);
      }
    };
    var thrInput = document.getElementById('rf-tf-thr');
    thrInput.oninput = function (e) { state.testThresholdPct = e.target.value; };

    var clrBtn = document.getElementById('rf-tf-clear');
    if (clrBtn) clrBtn.onclick = clearTestFaceFile;

    var prevImg = document.getElementById('rf-tf-img');
    if (prevImg) prevImg.onclick = function () { openViewer(state.testPreview); };

    var drop = document.getElementById('rf-tf-drop');
    drop.ondragover = function (e) { e.preventDefault(); drop.classList.add('dragover'); };
    drop.ondragleave = function () { drop.classList.remove('dragover'); };
    drop.ondrop = function (e) {
      e.preventDefault();
      drop.classList.remove('dragover');
      var f = e.dataTransfer && e.dataTransfer.files && e.dataTransfer.files[0];
      if (f && (!f.type || f.type.indexOf('image/') === 0)) {
        setTestFaceFile(f);
        toast('Image loaded');
      } else {
        toast('Drop a valid JPG, PNG, WEBP, or BMP image', false);
      }
    };

    document.getElementById('rf-tf-paste').onclick = async function () {
      if (!navigator.clipboard || typeof navigator.clipboard.read !== 'function') {
        toast('Press Ctrl+V to paste a copied image from your clipboard', true);
        return;
      }
      try {
        var items = await navigator.clipboard.read();
        for (var i = 0; i < items.length; i++) {
          var imgType = items[i].types.find(function (t) { return t.indexOf('image/') === 0; });
          if (imgType) {
            var blob = await items[i].getType(imgType);
            setTestFaceFile(blob, 'clipboard-image');
            toast('Image pasted from clipboard');
            return;
          }
        }
        toast('No image found in clipboard. Copy an image first or press Ctrl+V.', false);
      } catch (err) {
        toast('Clipboard permission needed — press Ctrl+V directly to paste your image', true);
      }
    };

    async function runRecognition() {
      var f = state.testFile || (fileInput.files && fileInput.files[0]);
      if (!f) { toast('Choose or paste a photo first', false); return; }
      var pct = Math.min(99, Math.max(50, Number(thrInput.value || state.testThresholdPct || 88)));
      var thrDecimal = (pct / 100).toFixed(2);
      var fd = new FormData();
      fd.append('image', f, state.testFileName || f.name || ('pasted.' + guessExtFromMime(f.type)));
      fd.append('threshold', thrDecimal);
      var runBtn = document.getElementById('rf-tf-run');
      runBtn.disabled = true;
      state.testUnrecognized = false;
      renderTestEnrollPrompt(enrollContainer, body);
      document.getElementById('rf-tf-out').innerHTML = '⏳ Running face detection and vector matching…';
      try {
        var r = await api('/admin/api/test-recognition', { method: 'POST', body: fd });
        if (!r.ok) {
          toast((r.data && r.data.reason) || 'Test failed', false);
          document.getElementById('rf-tf-out').innerHTML = '<div class="rf-err-banner">' + esc((r.data && r.data.reason) || 'Recognition test failed.') + '</div>';
          return;
        }
        var dets = r.data.matches || r.data.detections || [];
        var hasUnknownFace = dets.some(function (x) {
          return (!x.identity || x.identity === 'Unknown') && !x.reason;
        }) || (dets.length > 0 && dets.every(function (x) { return !x.identity || x.identity === 'Unknown'; }));

        state.testUnrecognized = hasUnknownFace;
        state.testResultHtml = dets.length ? '<div class="rf-grid">' + dets.map(function (x, i) {
          var ok = x.identity && x.identity !== 'Unknown';
          var confPct = (Number(x.confidence || 0) * 100).toFixed(1) + '%';
          return '<div class="rf-card rf-card-col"><h3>Face ' + (i + 1) + ' <span class="rf-badge ' + (ok ? 'rf-badge-ok' : 'rf-badge-warn') + '">' + (ok ? 'MATCHED' : 'UNKNOWN') + '</span></h3>' +
            '<div><b>Identity:</b> ' + esc(x.identity || 'Unknown') + '</div>' +
            '<div><b>Confidence:</b> ' + confPct + '</div></div>';
        }).join('') + '</div>' : '<div class="rf-empty">No face detected in this image.</div>';
        document.getElementById('rf-tf-out').innerHTML = state.testResultHtml;
        renderTestEnrollPrompt(enrollContainer, body);
      } catch (err) {
        toast('Error: ' + err.message, false);
        document.getElementById('rf-tf-out').innerHTML = '<div class="rf-err-banner">' + esc(err.message) + '</div>';
      } finally {
        runBtn.disabled = false;
      }
    }

    document.getElementById('rf-tf-run').onclick = runRecognition;
    var retestBtn = document.getElementById('rf-te-retest');
    if (retestBtn) retestBtn.onclick = runRecognition;
  }

  // ─── Mount Button Directly Under MCP Settings in LibreChat Rail ───────────
  function createAdminButton() {
    var btn = document.createElement('button');
    btn.type = 'button';
    btn.id = BTN_ID;
    btn.setAttribute('data-testid', 'nav-panel-arkib-admin');
    btn.setAttribute('aria-label', 'Admin Portal');
    btn.setAttribute('aria-pressed', state.open ? 'true' : 'false');
    btn.className = 'rf-admin-rail-btn flex h-9 w-9 items-center justify-center rounded-lg transition-colors' + (state.open ? ' rf-active' : '');
    btn.innerHTML =
      '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="h-5 w-5" aria-hidden="true">' +
      '<path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10"/>' +
      '<path d="m9 12 2 2 4-4"/>' +
      '</svg>' +
      '<span class="rf-rail-tooltip">Admin Portal</span>';
    btn.addEventListener('mouseenter', function () {
      var tip = btn.querySelector('.rf-rail-tooltip');
      if (!tip) return;
      var rect = btn.getBoundingClientRect();
      tip.style.top = Math.round(rect.top + rect.height / 2 - (tip.offsetHeight || 28) / 2) + 'px';
      tip.style.left = Math.round(rect.right + 12) + 'px';
    });
    btn.addEventListener('click', function (ev) {
      ev.preventDefault();
      ev.stopPropagation();
      if (state.open) closeAdmin();
      else openAdmin();
    });
    return btn;
  }

  function mountAdminButton() {
    var mcpBtn = document.querySelector('[data-testid="nav-panel-mcp-builder"]');
    if (!mcpBtn || !mcpBtn.parentElement) return;
    state.railEl = mcpBtn.closest('.bg-surface-sidebar') || mcpBtn.parentElement.parentElement;
    var existing = document.getElementById(BTN_ID);
    if (existing) {
      if (existing.previousElementSibling !== mcpBtn) mcpBtn.insertAdjacentElement('afterend', existing);
      return;
    }
    mcpBtn.insertAdjacentElement('afterend', createAdminButton());
    if (state.open) positionStage();
  }

  document.addEventListener('click', function (ev) {
    if (!ev.target.closest || (!ev.target.closest('.rf-menu-pop') && !ev.target.closest('.rf-folder-act'))) {
      closeMenus();
    }
    if (!state.open) return;
    var target = ev.target && ev.target.closest ? ev.target.closest('button, a') : null;
    if (!target || target.id === BTN_ID) return;
    var tid = target.getAttribute('data-testid') || '';
    if (tid.indexOf('nav-panel-') === 0 || tid === 'new-chat-button' || tid === 'open-sidebar-button') {
      closeAdmin();
    }
  }, true);

  document.addEventListener('keydown', function (ev) {
    if (ev.key === 'Escape') {
      if (document.querySelector('.rf-menu-pop')) { closeMenus(); return; }
      var modals = document.querySelectorAll('.rf-modal-backdrop');
      if (modals.length) { modals[modals.length - 1].remove(); return; }
      if (state.open) closeAdmin();
    }
  });

  document.addEventListener('paste', function (ev) {
    if (!state.open || !state.authed || state.tab !== 'test') return;
    if (document.querySelector('.rf-modal-backdrop')) return;
    var cd = ev.clipboardData || window.clipboardData;
    if (!cd || !cd.items) return;
    for (var i = 0; i < cd.items.length; i++) {
      var item = cd.items[i];
      if (item && item.type && item.type.indexOf('image/') === 0) {
        var f = item.getAsFile();
        if (f) {
          ev.preventDefault();
          setTestFaceFile(f, 'clipboard-image');
          toast('Image pasted from clipboard');
          return;
        }
      }
    }
  });

  window.addEventListener('resize', function () { if (state.open) positionStage(); });

  var domObserver = new MutationObserver(mountAdminButton);
  domObserver.observe(document.documentElement, { childList: true, subtree: true });

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', mountAdminButton);
  else mountAdminButton();

  if (window.location.hash === '#arkib-admin' || window.location.hash === '#rojakface-admin') {
    setTimeout(openAdmin, 450);
  }
})();


// ═════════════════════════════════════════════════════════════════════════════
// Arkib Canvas — Split-Screen Right-Side Drawer (Zero-LLM-Token Native UI)
// Mode 1: Jawi & Manuscript OCR Workbench (`ocr`)
// Mode 2: Identity & Photo Enrollment Canvas (`enroll`)
// ═════════════════════════════════════════════════════════════════════════════
(function () {
  var API_ORIGIN = (window.location.hostname === '127.0.0.1' || window.location.hostname === 'localhost')
    ? ('http://' + window.location.hostname + ':8005')
    : 'http://127.0.0.1:8005';
  var DRAWER_ID = 'ac-drawer';
  var RAIL_BTN_ID = 'ac-canvas-rail-btn';

  var ac = {
    open: false,
    mode: 'ocr',
    sessionId: 'latest',
    widthVw: 46,
    isAdmin: false,
    data: null,
    // Mode 1 (OCR) state
    ocrZoom: 1,
    ocrPanX: 0,
    ocrPanY: 0,
    activeStripIdx: -1,
    jawiText: '',
    rumiText: '',
    rumiManualOverride: false,
    lexiconPairs: [],
    previewTimer: null,
    // Mode 2 (Enroll) state
    enrollImg: null,
    enrollImgSrc: '',
    rotateDeg: 0,
    brightness: 100,
    contrast: 100,
    cropRect: null, // { x, y, w, h } in canvas CSS coordinates
    croppedDataUrl: '',
    enrollMode: 'new', // 'new' | 'existing'
    dragMode: null,
    dragStart: null,
    cropStart: null
  };

  function esc(s) {
    return String(s == null ? '' : s)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
  }

  function apiUrl(path) {
    if (!path) return '';
    if (path.indexOf('http://') === 0 || path.indexOf('https://') === 0 || path.indexOf('data:') === 0) return path;
    return API_ORIGIN + path;
  }

  async function acApi(path, opts) {
    var o = Object.assign({ credentials: 'include' }, opts || {});
    var resp = await fetch(API_ORIGIN + path, o);
    var data = {};
    try { data = await resp.json(); } catch (_) { data = {}; }
    return { ok: resp.ok, status: resp.status, data: data };
  }

  function acToast(msg, ok) {
    var el = document.createElement('div');
    el.className = 'rf-toast' + (ok === false ? ' rf-toast-err' : '');
    el.textContent = msg;
    document.body.appendChild(el);
    setTimeout(function () { el.remove(); }, 3800);
  }

  function parseCanvasHash(raw) {
    if (!raw) return null;
    var idx = raw.indexOf('#arkib-canvas:');
    if (idx === -1) return null;
    var frag = raw.slice(idx + '#arkib-canvas:'.length);
    var parts = frag.split(':');
    var mode = (parts[0] === 'enroll') ? 'enroll' : 'ocr';
    var id = (parts[1] || 'latest').replace(/[^a-zA-Z0-9_-]/g, '') || 'latest';
    return { mode: mode, id: id };
  }

  function applyDrawerWidth(pxOrVw) {
    var val = typeof pxOrVw === 'number' ? (pxOrVw + 'vw') : String(pxOrVw);
    document.documentElement.style.setProperty('--ac-width', val);
  }

  function ensureDrawer() {
    var el = document.getElementById(DRAWER_ID);
    if (el) return el;
    el = document.createElement('aside');
    el.id = DRAWER_ID;
    el.setAttribute('aria-label', 'Arkib Canvas');
    el.innerHTML =
      '<div class="ac-resize-handle" id="ac-resize-handle" title="Drag to resize Canvas"></div>' +
      '<div class="ac-header">' +
      '  <div class="ac-title-wrap">' +
      '    <span>🗂️ Arkib Canvas</span>' +
      '    <span class="ac-role-pill" id="ac-role-pill">Guest · Staged Approval</span>' +
      '  </div>' +
      '  <div style="display:flex;align-items:center;gap:6px">' +
      '    <button type="button" class="rf-btn" id="ac-reload-btn" style="padding:4px 9px;font-size:.74rem" title="Reload session">↻</button>' +
      '    <button type="button" class="rf-btn" id="ac-close-btn" style="padding:4px 9px;font-size:.78rem" aria-label="Close Arkib Canvas">✕</button>' +
      '  </div>' +
      '</div>' +
      '<div class="ac-mode-tabs">' +
      '  <button type="button" class="ac-mode-tab" data-ac-mode="ocr">📜 Jawi &amp; OCR Workbench</button>' +
      '  <button type="button" class="ac-mode-tab" data-ac-mode="enroll">🪪 Identity Enrollment</button>' +
      '</div>' +
      '<div class="ac-body" id="ac-body"><div class="rf-empty">Loading Arkib Canvas…</div></div>' +
      '<div class="ac-footer" id="ac-footer"></div>';
    document.body.appendChild(el);

    document.getElementById('ac-close-btn').onclick = closeCanvas;
    document.getElementById('ac-reload-btn').onclick = function () {
      loadSession(ac.mode, ac.sessionId);
    };
    el.querySelectorAll('[data-ac-mode]').forEach(function (btn) {
      btn.onclick = function () {
        var nextMode = btn.getAttribute('data-ac-mode');
        if (nextMode !== ac.mode) {
          openCanvas(nextMode, 'latest');
        }
      };
    });

    // Draggable left-edge resize handle (360px .. 75vw)
    var handle = document.getElementById('ac-resize-handle');
    handle.addEventListener('mousedown', function (ev) {
      ev.preventDefault();
      document.body.classList.add('ac-canvas-resizing');
      var onMove = function (e) {
        var vw = ((window.innerWidth - e.clientX) / window.innerWidth) * 100;
        var minVw = (360 / window.innerWidth) * 100;
        ac.widthVw = Math.max(minVw, Math.min(75, vw));
        applyDrawerWidth(ac.widthVw.toFixed(2) + 'vw');
      };
      var onUp = function () {
        document.body.classList.remove('ac-canvas-resizing');
        window.removeEventListener('mousemove', onMove);
        window.removeEventListener('mouseup', onUp);
      };
      window.addEventListener('mousemove', onMove);
      window.addEventListener('mouseup', onUp);
    });

    return el;
  }

  function updateRailButtonState() {
    var b = document.getElementById(RAIL_BTN_ID);
    if (!b) return;
    b.classList.toggle('ac-active', ac.open);
    b.setAttribute('aria-pressed', ac.open ? 'true' : 'false');
  }

  function closeCanvas() {
    ac.open = false;
    document.body.classList.remove('ac-canvas-open');
    var el = document.getElementById(DRAWER_ID);
    if (el) el.remove();
    updateRailButtonState();
    if (window.location.hash && window.location.hash.indexOf('#arkib-canvas:') === 0) {
      try { history.replaceState(null, '', window.location.pathname + window.location.search); } catch (_) {}
    }
  }

  async function openCanvas(mode, id) {
    ac.mode = (mode === 'enroll') ? 'enroll' : 'ocr';
    ac.sessionId = id || 'latest';
    ac.open = true;
    applyDrawerWidth(ac.widthVw + 'vw');
    document.body.classList.add('ac-canvas-open');
    ensureDrawer();
    updateRailButtonState();
    await loadSession(ac.mode, ac.sessionId);
  }

  async function loadSession(mode, id) {
    var el = ensureDrawer();
    el.querySelectorAll('[data-ac-mode]').forEach(function (b) {
      b.classList.toggle('ac-active', b.getAttribute('data-ac-mode') === mode);
    });
    var body = document.getElementById('ac-body');
    var footer = document.getElementById('ac-footer');
    body.innerHTML = '<div class="rf-empty">Loading ' + (mode === 'ocr' ? 'Jawi OCR Workbench' : 'Enrollment Canvas') + '…</div>';
    footer.innerHTML = '';

    try {
      var r = await acApi('/canvas/api/session/' + encodeURIComponent(mode) + '/' + encodeURIComponent(id || 'latest'));
      if (!r.ok || !r.data) {
        body.innerHTML = '<div class="rf-err-banner">Could not load Arkib Canvas session (' + esc(r.data && r.data.reason || r.status) + ').</div>';
        return;
      }
      ac.data = r.data;
      ac.isAdmin = !!r.data.is_admin;
      ac.sessionId = r.data.session_id || id || 'new';
      var pill = document.getElementById('ac-role-pill');
      if (pill) {
        pill.className = 'ac-role-pill' + (ac.isAdmin ? ' ac-admin' : '');
        pill.textContent = ac.isAdmin ? 'Admin · Direct Save' : 'Guest · Staged Approval';
      }
      if (mode === 'ocr') {
        ac.ocrZoom = 1;
        ac.ocrPanX = 0;
        ac.ocrPanY = 0;
        ac.activeStripIdx = -1;
        ac.jawiText = String(r.data.raw_jawi_text || '');
        ac.rumiText = String(r.data.standard_malay_rumi_text || '');
        ac.rumiManualOverride = false;
        ac.lexiconPairs = [];
        renderOcrWorkbench(body, footer);
      } else {
        ac.rotateDeg = 0;
        ac.brightness = 100;
        ac.contrast = 100;
        ac.cropRect = null;
        ac.croppedDataUrl = '';
        ac.enrollImg = null;
        ac.enrollImgSrc = r.data.image_url ? apiUrl(r.data.image_url) : '';
        ac.enrollMode = 'new';
        renderEnrollCanvas(body, footer);
      }
    } catch (err) {
      body.innerHTML = '<div class="rf-err-banner">Could not reach local ArkibVision server (port 8005): ' + esc(err.message) + '</div>';
    }
  }

  // ─── Mode 1: Jawi & Manuscript OCR Workbench ──────────────────────────────
  function renderOcrWorkbench(body, footer) {
    var d = ac.data || {};
    var chunks = Array.isArray(d.chunks) ? d.chunks : [];
    var docs = Array.isArray(d.documents) ? d.documents : [];
    var activeImgUrl = ac.activeStripIdx >= 0 && chunks[ac.activeStripIdx]
      ? chunks[ac.activeStripIdx].enhanced_chunk_url
      : (d.image_url ? apiUrl(d.image_url) : '');

    var stripHtml = chunks.length
      ? '<div class="ac-strip-list">' +
        '<button type="button" class="ac-strip-btn' + (ac.activeStripIdx === -1 ? ' ac-active' : '') + '" data-ac-strip="-1">Full Scan</button>' +
        chunks.map(function (c, i) {
          var badge = c.has_rubrication ? '<span class="ac-badge-red">Red Ink</span>' : '';
          return '<button type="button" class="ac-strip-btn' + (ac.activeStripIdx === i ? ' ac-active' : '') + '" data-ac-strip="' + i + '">' +
            esc(c.region_label || ('Strip ' + (i + 1))) + ' (' + esc(c.detected_text_lines || 1) + 'L) ' + badge + '</button>';
        }).join('') +
        '</div>'
      : '';

    var cov = typeof d.coverage_pct === 'number' ? d.coverage_pct.toFixed(1) + '%' : '—';
    var styleLbl = d.script_style === 'old_classical_jawi_or_mixed' ? 'Old Classical Jawi' : 'Modern Jawi';
    var hijri = Array.isArray(d.hijri_colophon_dates) ? d.hijri_colophon_dates : [];
    var hijriHtml = hijri.length
      ? '<div id="ac-hijri-bar" style="font-size:.74rem;color:#fbbf24;padding:4px 8px;border-radius:6px;background:rgba(245,158,11,.1)">📅 Colophon Date: ' +
        hijri.map(function (h) { return esc(h.hijri_date) + ' → ' + esc(h.gregorian_date_masihi); }).join(' · ') + '</div>'
      : '<div id="ac-hijri-bar" style="display:none"></div>';

    body.innerHTML =
      (activeImgUrl
        ? '<div class="ac-card">' +
          '  <div class="ac-card-head">' +
          '    <span>🖼️ Scan Viewer ' + (d.file ? '· ' + esc(d.file) : '') + (d.has_rubrication ? ' <span class="ac-badge-red">Dakwat Merah</span>' : '') + '</span>' +
          '    <div style="display:flex;gap:4px">' +
          '      <button type="button" class="rf-btn" id="ac-zoom-out" style="padding:2px 8px;font-size:.74rem">−</button>' +
          '      <button type="button" class="rf-btn" id="ac-zoom-reset" style="padding:2px 8px;font-size:.72rem">100%</button>' +
          '      <button type="button" class="rf-btn" id="ac-zoom-in" style="padding:2px 8px;font-size:.74rem">+</button>' +
          '    </div>' +
          '  </div>' +
          '  <div class="ac-scan-viewport" id="ac-scan-vp"><img id="ac-scan-img" src="' + esc(activeImgUrl) + '" alt="Manuscript scan"/></div>' +
          stripHtml +
          '</div>'
        : '') +
      '<div class="ac-card">' +
      '  <div class="ac-card-head">' +
      '    <span>✍️ Jawi Editor (RTL)</span>' +
      '    <div class="ac-jawi-keys">' +
      ['چ', 'ڠ', 'ڤ', 'ݢ', 'ڽ', 'ۏ'].map(function (ch) {
        return '<button type="button" class="ac-jawi-key" data-ac-jawi="' + ch + '" title="Insert ' + ch + '">' + ch + '</button>';
      }).join('') +
      '    </div>' +
      '  </div>' +
      '  <textarea id="ac-jawi-ta" class="ac-jawi-textarea" dir="rtl" placeholder="توليس اتاو سونتيڠ تيکس جاوي دسيني...">' + esc(ac.jawiText) + '</textarea>' +
      '</div>' +
      '<div class="ac-card">' +
      '  <div class="ac-card-head">' +
      '    <span>🇲🇾 Live Standard Bahasa Malaysia (Rumi)</span>' +
      '    <div style="display:flex;align-items:center;gap:6px">' +
      '      <span class="rf-meta" id="ac-style-lbl">' + esc(styleLbl) + '</span>' +
      '      <span class="ac-badge-cov" id="ac-cov-badge">FST Coverage: ' + esc(cov) + '</span>' +
      '    </div>' +
      '  </div>' +
      hijriHtml +
      '  <textarea id="ac-rumi-ta" class="ac-rumi-textarea" placeholder="Deterministic Rumi transliteration appears here automatically (<5ms)…">' + esc(ac.rumiText) + '</textarea>' +
      '</div>' +
      '<div class="ac-card">' +
      '  <div class="ac-card-head"><span>📚 Document Link &amp; DBP Lexicon Suggestions</span></div>' +
      '  <div class="ac-form-grid">' +
      '    <div class="ac-field"><label>Link to Existing Document (optional)</label>' +
      '      <select id="ac-doc-slug" class="rf-input" style="width:100%">' +
      '        <option value="">— Create / Unlinked Transcription —</option>' +
      docs.map(function (doc) {
        var sel = (d.document_slug && d.document_slug === doc.slug) ? ' selected' : '';
        return '<option value="' + esc(doc.slug) + '"' + sel + '>' + esc(doc.title || doc.slug) + '</option>';
      }).join('') +
      '      </select>' +
      '    </div>' +
      '    <div class="ac-field"><label>Title / Notes</label><input id="ac-ocr-title" class="rf-input" style="width:100%" placeholder="e.g. Surat Sultan 1324H" value="' + esc(d.file || '') + '"/></div>' +
      '  </div>' +
      '  <div style="display:flex;gap:6px;align-items:center;margin-top:4px">' +
      '    <input id="ac-lex-jawi" class="rf-input" dir="rtl" style="flex:1" placeholder="Jawi word (e.g. ڤرلمباݢأن)"/>' +
      '    <input id="ac-lex-rumi" class="rf-input" style="flex:1" placeholder="DBP Rumi (e.g. Perlembagaan)"/>' +
      '    <button type="button" class="rf-btn" id="ac-lex-add">+ Pair</button>' +
      '  </div>' +
      '  <div id="ac-lex-list" style="display:flex;flex-wrap:wrap;gap:6px;font-size:.74rem"></div>' +
      '  <input id="ac-ocr-notes" class="rf-input" style="width:100%" placeholder="Archivist notes (optional)…"/>' +
      '</div>';

    footer.innerHTML =
      '<span style="font-size:.74rem;color:rgb(var(--text-secondary))">' +
      (ac.isAdmin ? 'Admin session active — saves directly to archive &amp; DBP TSV.' : 'Guest mode — creates staged REQ-* for Admin approval.') +
      '</span>' +
      '<button type="button" class="rf-btn rf-btn-primary" id="ac-ocr-submit">' +
      (ac.isAdmin ? '✅ Save Verified Transcription' : '📤 Submit Correction for Approval') +
      '</button>';

    // Scan zoom/pan bindings
    var img = document.getElementById('ac-scan-img');
    var vp = document.getElementById('ac-scan-vp');
    var updateImgTransform = function () {
      if (img) img.style.transform = 'translate(' + ac.ocrPanX + 'px,' + ac.ocrPanY + 'px) scale(' + ac.ocrZoom + ')';
      var zr = document.getElementById('ac-zoom-reset');
      if (zr) zr.textContent = Math.round(ac.ocrZoom * 100) + '%';
    };
    if (vp && img) {
      document.getElementById('ac-zoom-in').onclick = function () { ac.ocrZoom = Math.min(4, ac.ocrZoom + 0.35); updateImgTransform(); };
      document.getElementById('ac-zoom-out').onclick = function () { ac.ocrZoom = Math.max(0.6, ac.ocrZoom - 0.35); updateImgTransform(); };
      document.getElementById('ac-zoom-reset').onclick = function () { ac.ocrZoom = 1; ac.ocrPanX = 0; ac.ocrPanY = 0; updateImgTransform(); };
      vp.addEventListener('mousedown', function (ev) {
        var sx = ev.clientX - ac.ocrPanX;
        var sy = ev.clientY - ac.ocrPanY;
        var onM = function (e) { ac.ocrPanX = e.clientX - sx; ac.ocrPanY = e.clientY - sy; updateImgTransform(); };
        var onU = function () { window.removeEventListener('mousemove', onM); window.removeEventListener('mouseup', onU); };
        window.addEventListener('mousemove', onM);
        window.addEventListener('mouseup', onU);
      });
    }
    body.querySelectorAll('[data-ac-strip]').forEach(function (btn) {
      btn.onclick = function () {
        ac.activeStripIdx = parseInt(btn.getAttribute('data-ac-strip'), 10);
        ac.ocrZoom = 1; ac.ocrPanX = 0; ac.ocrPanY = 0;
        renderOcrWorkbench(body, footer);
      };
    });

    // Jawi editor + 6-key helper bar + debounced 250ms deterministic preview
    var jawiTa = document.getElementById('ac-jawi-ta');
    var rumiTa = document.getElementById('ac-rumi-ta');
    var runPreview = async function () {
      var text = jawiTa.value;
      ac.jawiText = text;
      if (!text.trim()) {
        if (!ac.rumiManualOverride) { rumiTa.value = ''; ac.rumiText = ''; }
        return;
      }
      try {
        var r = await acApi('/canvas/api/jawi-preview', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ jawi_text: text })
        });
        if (r.ok && r.data) {
          if (!ac.rumiManualOverride) {
            ac.rumiText = r.data.standard_malay_rumi_text || '';
            rumiTa.value = ac.rumiText;
          }
          var covEl = document.getElementById('ac-cov-badge');
          if (covEl && typeof r.data.coverage_pct === 'number') covEl.textContent = 'FST Coverage: ' + r.data.coverage_pct.toFixed(1) + '%';
          var stEl = document.getElementById('ac-style-lbl');
          if (stEl) stEl.textContent = r.data.script_style === 'old_classical_jawi_or_mixed' ? 'Old Classical Jawi' : 'Modern Jawi';
          var hb = document.getElementById('ac-hijri-bar');
          var hd = Array.isArray(r.data.hijri_colophon_dates) ? r.data.hijri_colophon_dates : [];
          if (hb) {
            if (hd.length) {
              hb.style.display = 'block';
              hb.style.cssText = 'font-size:.74rem;color:#fbbf24;padding:4px 8px;border-radius:6px;background:rgba(245,158,11,.1)';
              hb.innerHTML = '📅 Colophon Date: ' + hd.map(function (h) { return esc(h.hijri_date) + ' → ' + esc(h.gregorian_date_masihi); }).join(' · ');
            } else {
              hb.style.display = 'none';
            }
          }
        }
      } catch (_) {}
    };
    var schedulePreview = function () {
      ac.rumiManualOverride = false;
      clearTimeout(ac.previewTimer);
      ac.previewTimer = setTimeout(runPreview, 250);
    };
    jawiTa.addEventListener('input', schedulePreview);
    rumiTa.addEventListener('input', function () {
      ac.rumiText = rumiTa.value;
      ac.rumiManualOverride = true;
    });

    body.querySelectorAll('[data-ac-jawi]').forEach(function (kbtn) {
      kbtn.onclick = function () {
        var ch = kbtn.getAttribute('data-ac-jawi');
        var start = jawiTa.selectionStart || 0;
        var end = jawiTa.selectionEnd || 0;
        var v = jawiTa.value;
        jawiTa.value = v.slice(0, start) + ch + v.slice(end);
        jawiTa.focus();
        jawiTa.selectionStart = jawiTa.selectionEnd = start + ch.length;
        schedulePreview();
      };
    });

    // Lexicon suggestion pairs
    var renderLexList = function () {
      var listEl = document.getElementById('ac-lex-list');
      if (!listEl) return;
      listEl.innerHTML = ac.lexiconPairs.map(function (p, i) {
        return '<span class="ac-strip-btn">' + esc(p.jawi) + ' → ' + esc(p.rumi) +
          ' <b style="cursor:pointer;color:#f87171;margin-left:3px" data-ac-lex-rm="' + i + '">×</b></span>';
      }).join('');
      listEl.querySelectorAll('[data-ac-lex-rm]').forEach(function (rm) {
        rm.onclick = function () {
          ac.lexiconPairs.splice(parseInt(rm.getAttribute('data-ac-lex-rm'), 10), 1);
          renderLexList();
        };
      });
    };
    document.getElementById('ac-lex-add').onclick = function () {
      var j = document.getElementById('ac-lex-jawi').value.trim();
      var r = document.getElementById('ac-lex-rumi').value.trim();
      if (!j || !r) { acToast('Enter both Jawi and DBP Rumi words.', false); return; }
      ac.lexiconPairs.push({ jawi: j, rumi: r });
      document.getElementById('ac-lex-jawi').value = '';
      document.getElementById('ac-lex-rumi').value = '';
      renderLexList();
    };
    renderLexList();

    // Submit OCR Edit
    document.getElementById('ac-ocr-submit').onclick = async function () {
      var btn = this;
      var jawiVal = jawiTa.value.trim();
      var rumiVal = rumiTa.value.trim();
      if (!jawiVal && !rumiVal) { acToast('Enter Jawi or Rumi text first.', false); return; }
      btn.disabled = true;
      try {
        var r = await acApi('/canvas/api/submit-ocr-edit', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            session_id: ac.sessionId,
            document_slug: document.getElementById('ac-doc-slug').value || null,
            document_title: document.getElementById('ac-ocr-title').value.trim() || null,
            edited_jawi_text: jawiVal,
            edited_rumi_text: rumiVal,
            lexicon_suggestions: ac.lexiconPairs,
            notes: document.getElementById('ac-ocr-notes').value.trim() || null
          })
        });
        if (r.ok && r.data) {
          if (r.data.status === 'saved_by_admin') {
            acToast('✅ Verified transcription saved (' + (r.data.document_slug || 'archive') + ', +' + (r.data.lexicon_added || 0) + ' lexicon pair(s)).', true);
          } else {
            acToast('📤 Submitted for Admin approval: ' + (r.data.request_id || 'REQ'), true);
          }
        } else {
          acToast('Error: ' + (r.data && r.data.reason || 'Submit failed'), false);
        }
      } catch (err) {
        acToast('Error: ' + err.message, false);
      } finally {
        btn.disabled = false;
      }
    };
  }

  // ─── Mode 2: Identity & Photo Enrollment Canvas (HTML5 <canvas> Editor) ───
  function renderEnrollCanvas(body, footer) {
    var d = ac.data || {};
    var people = Array.isArray(d.existing_identities) ? d.existing_identities : [];
    var groups = Array.isArray(d.people_groups) ? d.people_groups : [];

    body.innerHTML =
      '<div class="ac-card">' +
      '  <div class="ac-card-head">' +
      '    <span>✂️ Photo Editor &amp; Face Crop</span>' +
      '    <div style="display:flex;gap:5px;flex-wrap:wrap">' +
      '      <label class="rf-btn" style="padding:3px 8px;font-size:.72rem;cursor:pointer">📁 Upload<input type="file" id="ac-enr-file" accept="image/*" style="display:none"/></label>' +
      '      <button type="button" class="rf-btn" id="ac-rot-l" style="padding:3px 8px;font-size:.72rem">↺ 90°</button>' +
      '      <button type="button" class="rf-btn" id="ac-rot-r" style="padding:3px 8px;font-size:.72rem">↻ 90°</button>' +
      '      <button type="button" class="rf-btn" id="ac-crop-reset" style="padding:3px 8px;font-size:.72rem">Reset</button>' +
      '      <button type="button" class="rf-btn rf-btn-primary" id="ac-crop-apply" style="padding:3px 9px;font-size:.72rem">Apply Crop</button>' +
      '    </div>' +
      '  </div>' +
      '  <div class="ac-canvas-stage" id="ac-canvas-stage">' +
      '    <canvas id="ac-editor-canvas" width="420" height="260"></canvas>' +
      '    <div id="ac-no-img-hint" style="position:absolute;font-size:.78rem;color:rgb(var(--text-secondary));pointer-events:none">Upload or press Ctrl+V to paste a photo</div>' +
      '  </div>' +
      '  <div class="ac-slider-row">' +
      '    <label>Brightness (<span id="ac-bri-v">100%</span>)<input type="range" id="ac-bri" min="50" max="160" value="100"/></label>' +
      '    <label>Contrast (<span id="ac-con-v">100%</span>)<input type="range" id="ac-con" min="50" max="160" value="100"/></label>' +
      '  </div>' +
      '  <div id="ac-crop-status" style="font-size:.72rem;color:#34d399;display:none"></div>' +
      '</div>' +
      '<div class="ac-card">' +
      '  <div class="ac-card-head"><span>👤 Identity &amp; Group Folder</span></div>' +
      '  <div style="display:flex;gap:8px;margin-bottom:4px">' +
      '    <label style="display:flex;align-items:center;gap:5px;font-size:.78rem;cursor:pointer"><input type="radio" name="ac-enr-mode" value="new" checked/> New Person</label>' +
      '    <label style="display:flex;align-items:center;gap:5px;font-size:.78rem;cursor:pointer"><input type="radio" name="ac-enr-mode" value="existing"/> Existing Enrolled Person</label>' +
      '  </div>' +
      '  <div class="ac-form-grid">' +
      '    <div class="ac-field" id="ac-new-name-wrap"><label>Full Name *</label><input id="ac-enr-name" class="rf-input" style="width:100%" placeholder="e.g. Datuk Seri …" value="' + esc(d.suggested_identity || '') + '"/></div>' +
      '    <div class="ac-field" id="ac-exist-name-wrap" style="display:none"><label>Select Enrolled Person *</label>' +
      '      <select id="ac-enr-existing" class="rf-input" style="width:100%">' +
      '        <option value="">— Choose person —</option>' +
      people.map(function (n) { return '<option value="' + esc(n) + '">' + esc(n) + '</option>'; }).join('') +
      '      </select>' +
      '    </div>' +
      '    <div class="ac-field"><label>Group Folder</label>' +
      '      <select id="ac-enr-group" class="rf-input" style="width:100%">' +
      '        <option value="">Unassigned</option>' +
      groups.map(function (g) { return '<option value="' + esc(g.slug || g.name) + '">' + esc(g.name) + '</option>'; }).join('') +
      '      </select>' +
      '    </div>' +
      '  </div>' +
      '</div>' +
      '<div class="ac-card">' +
      '  <div class="ac-card-head"><span>🌐 Public Details (Visible in Chat)</span></div>' +
      '  <div class="ac-form-grid">' +
      '    <div class="ac-field"><label>Description / Role</label><input id="ac-pub-desc" class="rf-input" style="width:100%" placeholder="e.g. Archivist, Official Event"/></div>' +
      '    <div class="ac-field"><label>Location</label><input id="ac-pub-loc" class="rf-input" style="width:100%" placeholder="e.g. Arkib Negara Malaysia"/></div>' +
      '    <div class="ac-field"><label>Photograph By</label><input id="ac-pub-photog" class="rf-input" style="width:100%" placeholder="Photographer name"/></div>' +
      '    <div class="ac-field"><label>Date</label><input id="ac-pub-date" class="rf-input" style="width:100%" placeholder="YYYY-MM-DD"/></div>' +
      '  </div>' +
      '  <div class="ac-field"><label>Notes</label><input id="ac-enr-notes" class="rf-input" style="width:100%" placeholder="Optional notes…"/></div>' +
      '</div>' +
      '<div class="ac-private-box">' +
      '  <div class="ac-private-head"><span>🔒 Private / Restricted Details (Admin Only)</span><span style="font-size:.68rem;font-weight:500">Hidden from Chat</span></div>' +
      '  <div class="ac-form-grid">' +
      '    <div class="ac-field"><label>No. KP / NRIC</label><input id="ac-priv-nokp" class="rf-input" style="width:100%" placeholder="e.g. 880101-14-XXXX"/></div>' +
      '    <div class="ac-field"><label>No. Telefon / Phone</label><input id="ac-priv-phone" class="rf-input" style="width:100%" placeholder="e.g. +6012-XXXXXXX"/></div>' +
      '    <div class="ac-field"><label>Emel / Email</label><input id="ac-priv-email" class="rf-input" style="width:100%" placeholder="name@domain.my"/></div>' +
      '    <div class="ac-field"><label>Tarikh Lahir / DOB</label><input id="ac-priv-dob" class="rf-input" style="width:100%" placeholder="YYYY-MM-DD"/></div>' +
      '  </div>' +
      '  <div class="ac-field"><label>Alamat / Address</label><input id="ac-priv-addr" class="rf-input" style="width:100%" placeholder="Home address (restricted)"/></div>' +
      '</div>';

    footer.innerHTML =
      '<span style="font-size:.74rem;color:rgb(var(--text-secondary))">' +
      (ac.isAdmin ? 'Admin session — enrolls face vector &amp; profile directly.' : 'Guest mode — submits REQ-* for Admin approval.') +
      '</span>' +
      '<button type="button" class="rf-btn rf-btn-primary" id="ac-enr-submit">' +
      (ac.isAdmin ? '✅ Enroll Now' : '📤 Submit Enrollment for Approval') +
      '</button>';

    // Toggle New vs Existing Person
    body.querySelectorAll('input[name="ac-enr-mode"]').forEach(function (rad) {
      rad.addEventListener('change', function () {
        ac.enrollMode = rad.value;
        document.getElementById('ac-new-name-wrap').style.display = ac.enrollMode === 'new' ? 'block' : 'none';
        document.getElementById('ac-exist-name-wrap').style.display = ac.enrollMode === 'existing' ? 'block' : 'none';
      });
    });

    // HTML5 Canvas Editor
    var canvas = document.getElementById('ac-editor-canvas');
    var ctx = canvas.getContext('2d');
    var hintEl = document.getElementById('ac-no-img-hint');

    function drawEditor() {
      var img = ac.enrollImg;
      if (!img) {
        ctx.clearRect(0, 0, canvas.width, canvas.height);
        if (hintEl) hintEl.style.display = 'block';
        return;
      }
      if (hintEl) hintEl.style.display = 'none';
      var rot = ((ac.rotateDeg % 360) + 360) % 360;
      var swapped = (rot === 90 || rot === 270);
      var srcW = swapped ? img.naturalHeight : img.naturalWidth;
      var srcH = swapped ? img.naturalWidth : img.naturalHeight;
      var maxW = 460;
      var maxH = 300;
      var scale = Math.min(maxW / Math.max(1, srcW), maxH / Math.max(1, srcH), 1);
      var cw = Math.max(120, Math.round(srcW * scale));
      var ch = Math.max(100, Math.round(srcH * scale));
      if (canvas.width !== cw || canvas.height !== ch) {
        canvas.width = cw;
        canvas.height = ch;
      }

      ctx.save();
      ctx.clearRect(0, 0, cw, ch);
      ctx.filter = 'brightness(' + ac.brightness + '%) contrast(' + ac.contrast + '%)';
      ctx.translate(cw / 2, ch / 2);
      ctx.rotate((rot * Math.PI) / 180);
      var drawW = swapped ? ch : cw;
      var drawH = swapped ? cw : ch;
      ctx.drawImage(img, -drawW / 2, -drawH / 2, drawW, drawH);
      ctx.restore();

      if (!ac.cropRect) {
        // Pre-seed crop box around YuNet's detected face_box_xywh (with generous 35% margin) when unrotated
        var fb = d.face_box_xywh;
        if (Array.isArray(fb) && fb.length === 4 && rot === 0 && img.naturalWidth > 0) {
          var sx = cw / img.naturalWidth;
          var sy = ch / img.naturalHeight;
          var pad = Math.max(fb[2], fb[3]) * 0.35;
          var rx = Math.max(0, (fb[0] - pad) * sx);
          var ry = Math.max(0, (fb[1] - pad) * sy);
          var rw = Math.min(cw - rx, (fb[2] + pad * 2) * sx);
          var rh = Math.min(ch - ry, (fb[3] + pad * 2) * sy);
          ac.cropRect = { x: rx, y: ry, w: Math.max(40, rw), h: Math.max(40, rh) };
        } else {
          ac.cropRect = { x: Math.round(cw * 0.12), y: Math.round(ch * 0.08), w: Math.round(cw * 0.76), h: Math.round(ch * 0.84) };
        }
      }

      // Dim outside crop rectangle & draw crisp blue handles
      var c = ac.cropRect;
      ctx.save();
      ctx.fillStyle = 'rgba(2, 6, 23, 0.52)';
      ctx.fillRect(0, 0, cw, c.y);
      ctx.fillRect(0, c.y + c.h, cw, ch - (c.y + c.h));
      ctx.fillRect(0, c.y, c.x, c.h);
      ctx.fillRect(c.x + c.w, c.y, cw - (c.x + c.w), c.h);
      ctx.strokeStyle = '#3b82f6';
      ctx.lineWidth = 2;
      ctx.strokeRect(c.x, c.y, c.w, c.h);
      ctx.fillStyle = '#60a5fa';
      ctx.fillRect(c.x + c.w - 8, c.y + c.h - 8, 10, 10);
      ctx.restore();
    }

    function exportCroppedDataUrl() {
      var img = ac.enrollImg;
      if (!img) return '';
      var rot = ((ac.rotateDeg % 360) + 360) % 360;
      var swapped = (rot === 90 || rot === 270);
      var fullW = swapped ? img.naturalHeight : img.naturalWidth;
      var fullH = swapped ? img.naturalWidth : img.naturalHeight;
      var off = document.createElement('canvas');
      off.width = fullW;
      off.height = fullH;
      var octx = off.getContext('2d');
      octx.filter = 'brightness(' + ac.brightness + '%) contrast(' + ac.contrast + '%)';
      octx.translate(fullW / 2, fullH / 2);
      octx.rotate((rot * Math.PI) / 180);
      octx.drawImage(img, -img.naturalWidth / 2, -img.naturalHeight / 2);

      var c = ac.cropRect || { x: 0, y: 0, w: canvas.width, h: canvas.height };
      var scaleX = fullW / Math.max(1, canvas.width);
      var scaleY = fullH / Math.max(1, canvas.height);
      var sx = Math.max(0, Math.round(c.x * scaleX));
      var sy = Math.max(0, Math.round(c.y * scaleY));
      var sw = Math.max(24, Math.min(fullW - sx, Math.round(c.w * scaleX)));
      var sh = Math.max(24, Math.min(fullH - sy, Math.round(c.h * scaleY)));

      var out = document.createElement('canvas');
      var maxSide = 1024;
      var outScale = Math.min(1, maxSide / Math.max(sw, sh));
      out.width = Math.max(24, Math.round(sw * outScale));
      out.height = Math.max(24, Math.round(sh * outScale));
      out.getContext('2d').drawImage(off, sx, sy, sw, sh, 0, 0, out.width, out.height);
      return out.toDataURL('image/jpeg', 0.92);
    }

    function loadEditorImage(src) {
      var img = new Image();
      img.crossOrigin = 'use-credentials';
      img.onload = function () {
        ac.enrollImg = img;
        ac.cropRect = null;
        ac.croppedDataUrl = '';
        drawEditor();
      };
      img.onerror = function () {
        ac.enrollImg = null;
        drawEditor();
      };
      img.src = src;
    }

    if (ac.enrollImgSrc) loadEditorImage(ac.enrollImgSrc);
    else drawEditor();

    // Drag / resize crop box on canvas
    function canvasPos(ev) {
      var r = canvas.getBoundingClientRect();
      return {
        x: (ev.clientX - r.left) * (canvas.width / Math.max(1, r.width)),
        y: (ev.clientY - r.top) * (canvas.height / Math.max(1, r.height))
      };
    }
    canvas.addEventListener('mousedown', function (ev) {
      if (!ac.enrollImg || !ac.cropRect) return;
      var p = canvasPos(ev);
      var c = ac.cropRect;
      var nearCorner = Math.abs(p.x - (c.x + c.w)) <= 16 && Math.abs(p.y - (c.y + c.h)) <= 16;
      var inside = p.x >= c.x && p.x <= c.x + c.w && p.y >= c.y && p.y <= c.y + c.h;
      ac.dragMode = nearCorner ? 'resize' : (inside ? 'move' : 'new');
      ac.dragStart = p;
      ac.cropStart = { x: c.x, y: c.y, w: c.w, h: c.h };
    });
    window.addEventListener('mousemove', function (ev) {
      if (!ac.dragMode || !ac.open || ac.mode !== 'enroll') return;
      var p = canvasPos(ev);
      var s = ac.cropStart;
      var dx = p.x - ac.dragStart.x;
      var dy = p.y - ac.dragStart.y;
      if (ac.dragMode === 'move') {
        ac.cropRect = {
          x: Math.max(0, Math.min(canvas.width - s.w, s.x + dx)),
          y: Math.max(0, Math.min(canvas.height - s.h, s.y + dy)),
          w: s.w,
          h: s.h
        };
      } else if (ac.dragMode === 'resize') {
        ac.cropRect = {
          x: s.x,
          y: s.y,
          w: Math.max(36, Math.min(canvas.width - s.x, s.w + dx)),
          h: Math.max(36, Math.min(canvas.height - s.y, s.h + dy))
        };
      } else {
        var nx = Math.max(0, Math.min(ac.dragStart.x, p.x));
        var ny = Math.max(0, Math.min(ac.dragStart.y, p.y));
        ac.cropRect = {
          x: nx,
          y: ny,
          w: Math.max(32, Math.min(canvas.width - nx, Math.abs(dx))),
          h: Math.max(32, Math.min(canvas.height - ny, Math.abs(dy)))
        };
      }
      drawEditor();
    });
    window.addEventListener('mouseup', function () { ac.dragMode = null; });

    document.getElementById('ac-enr-file').addEventListener('change', function (e) {
      var f = e.target.files && e.target.files[0];
      if (!f) return;
      var reader = new FileReader();
      reader.onload = function () { loadEditorImage(reader.result); };
      reader.readAsDataURL(f);
    });
    document.getElementById('ac-rot-l').onclick = function () { ac.rotateDeg -= 90; ac.cropRect = null; drawEditor(); };
    document.getElementById('ac-rot-r').onclick = function () { ac.rotateDeg += 90; ac.cropRect = null; drawEditor(); };
    document.getElementById('ac-crop-reset').onclick = function () {
      ac.rotateDeg = 0; ac.brightness = 100; ac.contrast = 100; ac.cropRect = null; ac.croppedDataUrl = '';
      document.getElementById('ac-bri').value = 100; document.getElementById('ac-bri-v').textContent = '100%';
      document.getElementById('ac-con').value = 100; document.getElementById('ac-con-v').textContent = '100%';
      var st = document.getElementById('ac-crop-status'); if (st) st.style.display = 'none';
      drawEditor();
    };
    document.getElementById('ac-bri').addEventListener('input', function (e) {
      ac.brightness = parseInt(e.target.value, 10) || 100;
      document.getElementById('ac-bri-v').textContent = ac.brightness + '%';
      drawEditor();
    });
    document.getElementById('ac-con').addEventListener('input', function (e) {
      ac.contrast = parseInt(e.target.value, 10) || 100;
      document.getElementById('ac-con-v').textContent = ac.contrast + '%';
      drawEditor();
    });
    document.getElementById('ac-crop-apply').onclick = function () {
      if (!ac.enrollImg) { acToast('Load or paste a photo first.', false); return; }
      ac.croppedDataUrl = exportCroppedDataUrl();
      var st = document.getElementById('ac-crop-status');
      if (st) { st.style.display = 'block'; st.textContent = '✅ Crop & adjustments locked for submission.'; }
      acToast('Crop applied — ready to submit.', true);
    };

    // Allow Ctrl+V image paste straight into Enrollment Canvas
    ac._loadEditorImage = loadEditorImage;

    // Submit Enrollment
    document.getElementById('ac-enr-submit').onclick = async function () {
      var btn = this;
      var isExist = ac.enrollMode === 'existing';
      var existingId = isExist ? document.getElementById('ac-enr-existing').value.trim() : '';
      var newName = isExist ? '' : document.getElementById('ac-enr-name').value.trim();
      var target = existingId || newName;
      if (!target) { acToast('Please enter or select a person name.', false); return; }
      var imgB64 = ac.croppedDataUrl || (ac.enrollImg ? exportCroppedDataUrl() : '');
      if (!imgB64 && (!ac.sessionId || ac.sessionId === 'new')) {
        acToast('Upload or paste a photo first.', false);
        return;
      }

      var pub = {
        description: document.getElementById('ac-pub-desc').value.trim(),
        location: document.getElementById('ac-pub-loc').value.trim(),
        photograph_by: document.getElementById('ac-pub-photog').value.trim(),
        date: document.getElementById('ac-pub-date').value.trim()
      };
      var priv = {
        no_kp: document.getElementById('ac-priv-nokp').value.trim(),
        phone: document.getElementById('ac-priv-phone').value.trim(),
        email: document.getElementById('ac-priv-email').value.trim(),
        dob: document.getElementById('ac-priv-dob').value.trim(),
        address: document.getElementById('ac-priv-addr').value.trim()
      };

      btn.disabled = true;
      try {
        var r = await acApi('/canvas/api/submit-enrollment', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            session_id: ac.sessionId,
            image_base64: imgB64,
            identity_name: newName,
            existing_identity: existingId || null,
            group_folder: document.getElementById('ac-enr-group').value,
            public_fields: pub,
            private_fields: priv,
            notes: document.getElementById('ac-enr-notes').value.trim()
          })
        });
        if (r.ok && r.data) {
          if (r.data.is_admin) {
            acToast('✅ Enrolled directly: ' + (r.data.identity || target), true);
          } else {
            acToast('📤 Submitted for Admin approval: ' + (r.data.request_id || 'REQ') + ' (' + target + ')', true);
          }
        } else {
          acToast('Error: ' + (r.data && r.data.reason || 'Enrollment failed'), false);
        }
      } catch (err) {
        acToast('Error: ' + err.message, false);
      } finally {
        btn.disabled = false;
      }
    };
  }

  // ─── Rail Quick-Toggle Button & Global #arkib-canvas:<mode>:<id> Router ───
  function createCanvasRailButton() {
    var btn = document.createElement('button');
    btn.type = 'button';
    btn.id = RAIL_BTN_ID;
    btn.setAttribute('data-testid', 'nav-panel-arkib-canvas');
    btn.setAttribute('aria-label', 'Arkib Canvas');
    btn.setAttribute('aria-pressed', ac.open ? 'true' : 'false');
    btn.className = 'ac-rail-btn flex h-9 w-9 items-center justify-center rounded-lg transition-colors' + (ac.open ? ' ac-active' : '');
    btn.innerHTML =
      '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="h-5 w-5" aria-hidden="true">' +
      '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M12 3v18"/>' +
      '</svg>' +
      '<span class="rf-rail-tooltip">Arkib Canvas</span>';
    btn.addEventListener('mouseenter', function () {
      var tip = btn.querySelector('.rf-rail-tooltip');
      if (!tip) return;
      var rect = btn.getBoundingClientRect();
      tip.style.top = Math.round(rect.top + rect.height / 2 - (tip.offsetHeight || 28) / 2) + 'px';
      tip.style.left = Math.round(rect.right + 12) + 'px';
    });
    btn.addEventListener('click', function (ev) {
      ev.preventDefault();
      ev.stopPropagation();
      if (ac.open) closeCanvas();
      else openCanvas(ac.mode || 'ocr', 'latest');
    });
    return btn;
  }

  function mountCanvasButton() {
    var adminBtn = document.getElementById('rf-admin-rail-btn');
    if (!adminBtn || !adminBtn.parentElement) return;
    var existing = document.getElementById(RAIL_BTN_ID);
    if (existing) {
      if (existing.previousElementSibling !== adminBtn) adminBtn.insertAdjacentElement('afterend', existing);
      return;
    }
    adminBtn.insertAdjacentElement('afterend', createCanvasRailButton());
  }

  // Intercept any chat link matching `#arkib-canvas:<mode>:<id>`
  document.addEventListener('click', function (ev) {
    var a = ev.target && ev.target.closest ? ev.target.closest('a[href*="#arkib-canvas:"]') : null;
    if (!a) return;
    var parsed = parseCanvasHash(a.getAttribute('href') || '');
    if (!parsed) return;
    ev.preventDefault();
    ev.stopPropagation();
    openCanvas(parsed.mode, parsed.id);
  }, true);

  window.addEventListener('hashchange', function () {
    var parsed = parseCanvasHash(window.location.hash || '');
    if (parsed) openCanvas(parsed.mode, parsed.id);
  });

  document.addEventListener('paste', function (ev) {
    if (!ac.open || ac.mode !== 'enroll' || !ac._loadEditorImage) return;
    var cd = ev.clipboardData || window.clipboardData;
    if (!cd || !cd.items) return;
    for (var i = 0; i < cd.items.length; i++) {
      var item = cd.items[i];
      if (item && item.type && item.type.indexOf('image/') === 0) {
        var f = item.getAsFile();
        if (f) {
          ev.preventDefault();
          var reader = new FileReader();
          reader.onload = function () { ac._loadEditorImage(reader.result); acToast('Photo pasted into Enrollment Canvas', true); };
          reader.readAsDataURL(f);
          return;
        }
      }
    }
  });

  var acObserver = new MutationObserver(mountCanvasButton);
  acObserver.observe(document.documentElement, { childList: true, subtree: true });
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', mountCanvasButton);
  else mountCanvasButton();

  var initParsed = parseCanvasHash(window.location.hash || '');
  if (initParsed) setTimeout(function () { openCanvas(initParsed.mode, initParsed.id); }, 450);
})();
