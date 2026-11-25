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
    locations: [],
    documents: [],
    documentGroups: [],
    selectedDocGroup: 'all',
    health: null,
    integrity: null,
    labHtml: '',
    testResultHtml: '',
    testPreview: '',
    railEl: null,
    pollTimer: null,
    editingProfile: null,
    selectedPhoto: null,
    editingLoc: null,
    editingDoc: null
  };

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
        api('/admin/api/documents?ts=' + Date.now()),
        api('/admin/api/document-groups?ts=' + Date.now()),
        api('/admin/api/system/health?ts=' + Date.now())
      ]);
      if (res[0].ok && Array.isArray(res[0].data)) state.requests = res[0].data;
      if (res[1].ok && Array.isArray(res[1].data)) state.profiles = res[1].data;
      if (res[2].ok && Array.isArray(res[2].data)) state.peopleGroups = res[2].data;
      if (res[3].ok && Array.isArray(res[3].data)) state.locations = res[3].data;
      if (res[4].ok && Array.isArray(res[4].data)) state.documents = res[4].data;
      if (res[5].ok && Array.isArray(res[5].data)) state.documentGroups = res[5].data;
      if (res[6].ok) state.health = res[6].data;
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
        '<span class="rf-pill">🛡️ ROJAKFACE · ADMIN PORTAL</span>' +
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
          err.textContent = 'Could not reach local RojakFace service (port 8005).';
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
    var enrollCnt = pending.filter(function (x) { return x.request_type === 'enrollment'; }).length;
    var updateCnt = pending.filter(function (x) { return x.request_type === 'metadata_update'; }).length;
    var q = state.search.toLowerCase();
    var visible = pending.filter(function (x) {
      var mf = state.filter === 'all' || x.request_type === state.filter;
      var ms = !q || (String(x.name || '') + ' ' + String(x.id || '')).toLowerCase().indexOf(q) !== -1;
      return mf && ms;
    });

    body.innerHTML =
      '<div class="rf-section-head"><div><h2>Requests Awaiting Approval</h2><p>Review guest enrollment and profile update requests before adding them to the trusted archive.</p></div></div>' +
      '<div class="rf-stats">' +
      '<div class="rf-stat"><strong>' + pending.length + '</strong><span>Waiting for review</span></div>' +
      '<div class="rf-stat"><strong>' + enrollCnt + '</strong><span>New enrollments</span></div>' +
      '<div class="rf-stat"><strong>' + updateCnt + '</strong><span>Profile updates</span></div>' +
      '</div>' +
      '<div class="rf-toolbar">' +
      '<input class="rf-input" id="rf-req-search" style="flex:1;min-width:220px" type="search" placeholder="Search by name or request ID…" value="' + esc(state.search) + '"/>' +
      ['all', 'enrollment', 'metadata_update'].map(function (f) {
        var lbl = f === 'all' ? 'All' : f === 'enrollment' ? 'Enrollments' : 'Updates';
        return '<button type="button" class="rf-btn' + (state.filter === f ? ' rf-btn-primary' : '') + '" data-rf-flt="' + f + '">' + lbl + '</button>';
      }).join('') +
      '</div>' +
      '<div class="rf-grid">' +
      (visible.length ? visible.map(function (req) {
        var meta = req.metadata || {};
        var kind = req.request_type === 'metadata_update' ? 'Profile update' : 'New enrollment';
        var srcImg = apiUrl('/admin/images/source/' + encodeURIComponent(req.id));
        var cropImg = apiUrl('/admin/images/crop/' + encodeURIComponent(req.id));
        var rows = [
          ['Action', kind],
          ['Submitted', new Date(req.submitted_at || Date.now()).toLocaleString()],
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
        var runAct = async function () {
          b.disabled = true;
          try {
            var r = await api('/admin/api/' + act + '/' + encodeURIComponent(id), { method: 'POST' });
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

    var folderChips =
      '<div class="rf-folder-chip' + (state.selectedGroup === 'all' ? ' active' : '') + '" data-rf-grp="all">📂 All <span class="rf-fcnt">' + state.profiles.length + '</span></div>' +
      state.peopleGroups.map(function (g) {
        var gs = String(g.slug || '').toLowerCase();
        var cnt = state.profiles.filter(function (p) { return personGroupSlug(p) === gs; }).length;
        return '<div class="rf-folder-chip' + (state.selectedGroup === gs ? ' active' : '') + '" data-rf-grp="' + esc(gs) + '" title="' + esc(g.description || g.name) + '">' +
          '<span>📁 ' + esc(g.name) + '</span>' +
          '<span class="rf-fcnt">' + cnt + '</span>' +
          '<button type="button" class="rf-folder-act" data-rf-grp-menu="' + esc(gs) + '" aria-label="Folder options" title="Folder options">' + DOTS_SVG + '</button>' +
          '</div>';
      }).join('') +
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
    document.getElementById('rf-ppl-search').oninput = function (e) { state.search = e.target.value; renderPeople(body); };

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
    var endpoint = isDoc ? '/admin/api/document-groups' : '/admin/api/people-groups';
    var placeholder = isDoc
      ? 'e.g. Historical Manuscripts, Official Records, Treaties, Letters'
      : 'e.g. Staff, External Entity, VIP Guests, Researchers';
    var wrap = document.createElement('div');
    wrap.className = 'rf-modal-backdrop';
    wrap.innerHTML =
      '<div class="rf-modal" style="width:min(520px,100%)"><h2>' + (group ? 'Rename Group Folder' : (isDoc ? 'New Document Group Folder' : 'New People Group Folder')) + '</h2>' +
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
          if (isDoc && state.selectedDocGroup === String(group.slug || '').toLowerCase()) state.selectedDocGroup = nextSlug;
          if (!isDoc && state.selectedGroup === String(group.slug || '').toLowerCase()) state.selectedGroup = nextSlug;
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

  // ─── Tab 3: Archival Locations ────────────────────────────────────────────
  function renderLocations(body) {
    var q = state.search.toLowerCase();
    var list = state.locations.filter(function (l) { return !q || JSON.stringify(l).toLowerCase().indexOf(q) !== -1; });
    body.innerHTML =
      '<div class="rf-section-head"><div><h2>📍 Archival Locations &amp; Landmarks</h2><p>Manage specific halls, wings, rooms, and buildings (e.g. Arkib Negara Malaysia). Reference photos are automatically indexed for visual recognition.</p></div>' +
      '<button type="button" class="rf-btn rf-btn-primary" id="rf-add-loc-btn">＋ Add Location</button></div>' +
      '<div class="rf-toolbar"><input class="rf-input" id="rf-loc-search" style="flex:1;min-width:220px" type="search" placeholder="Search locations, wings, halls, or addresses…" value="' + esc(state.search) + '"/></div>' +
      '<div class="rf-grid">' +
      (list.length ? list.map(function (l) {
        var thumb = l.images && l.images[0] ? '<img class="rf-thumb" src="' + esc(apiUrl(l.images[0].admin_url)) + '" alt="' + esc(l.name) + '"/>' : '<div class="rf-thumb">📍</div>';
        var sub = [l.building_or_wing, l.floor_or_room].filter(Boolean).join(' · ') || l.parent_institution || l.category || 'Archival Location';
        return '<article class="rf-card">' + thumb + '<div class="rf-card-body">' +
          '<h3 title="' + esc(l.name) + '">' + esc(l.name) + '</h3>' +
          '<div class="rf-sub" title="' + esc(sub) + '">' + esc(sub) + '</div>' +
          '<p class="rf-desc">' + esc(l.description || l.historical_significance || 'No description yet.') + '</p>' +
          '<div class="rf-meta">' + (l.image_count || 0) + ' photo(s)' + (l.coordinates ? ' · ' + esc(l.coordinates) : '') + '</div>' +
          '<div class="rf-actions">' +
          '<button type="button" class="rf-btn rf-btn-primary rf-btn-sm" data-rf-edit-l="' + esc(l.slug) + '">' + EDIT_SVG + '<span>Edit</span></button>' +
          '<button type="button" class="rf-btn rf-btn-danger rf-btn-sm" data-rf-del-l="' + esc(l.slug) + '">' + TRASH_SVG + '<span>Delete</span></button>' +
          '</div></div></article>';
      }).join('') : '<div class="rf-empty">No archival locations yet.</div>') +
      '</div>';

    document.getElementById('rf-add-loc-btn').onclick = function () { openLocationModal(null); };
    document.getElementById('rf-loc-search').oninput = function (e) { state.search = e.target.value; renderLocations(body); };
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
    wrap.innerHTML =
      '<div class="rf-modal"><h2>' + (loc ? 'Edit Location' : 'Add Archival Location') + '</h2>' +
      '<form id="rf-l-form"><div class="rf-form-grid">' +
      '<div class="rf-field full"><label>Location Name</label><input class="rf-input" id="rf-l-name" required value="' + esc(loc ? loc.name || '' : '') + '"/></div>' +
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

    var folderChips =
      '<div class="rf-folder-chip' + (state.selectedDocGroup === 'all' ? ' active' : '') + '" data-rf-dgrp="all">📂 All <span class="rf-fcnt">' + state.documents.length + '</span></div>' +
      state.documentGroups.map(function (g) {
        var gs = String(g.slug || '').toLowerCase();
        var cnt = state.documents.filter(function (d) { return docGroupSlug(d) === gs; }).length;
        return '<div class="rf-folder-chip' + (state.selectedDocGroup === gs ? ' active' : '') + '" data-rf-dgrp="' + esc(gs) + '" title="' + esc(g.description || g.name) + '">' +
          '<span>📁 ' + esc(g.name) + '</span>' +
          '<span class="rf-fcnt">' + cnt + '</span>' +
          '<button type="button" class="rf-folder-act" data-rf-dgrp-menu="' + esc(gs) + '" aria-label="Folder options" title="Folder options">' + DOTS_SVG + '</button>' +
          '</div>';
      }).join('') +
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
    document.getElementById('rf-doc-search').oninput = function (e) { state.search = e.target.value; renderDocuments(body); };

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

  // ─── Tab 6: Test Face Recognition ─────────────────────────────────────────
  function renderTestFace(body) {
    body.innerHTML =
      '<div class="rf-section-head"><div><h2>🔍 Test Face Recognition</h2><p>Upload any photo to test face matching and confidence scores without modifying enrolled records.</p></div></div>' +
      '<div class="rf-box" style="margin-top:0">' +
      '<div class="rf-toolbar" style="margin-bottom:0">' +
      '<input class="rf-input" id="rf-tf-file" type="file" accept=".jpg,.jpeg,.png,.webp,.bmp"/>' +
      '<input class="rf-input" id="rf-tf-thr" type="number" min="0.50" max="0.99" step="0.01" value="0.88" style="width:110px" title="Minimum confidence"/>' +
      '<button type="button" class="rf-btn rf-btn-primary" id="rf-tf-run">Test Recognition</button>' +
      '</div>' +
      (state.testPreview ? '<img src="' + esc(state.testPreview) + '" style="margin-top:14px;max-height:320px;border-radius:10px;border:1px solid rgb(var(--border-medium));display:block"/>' : '') +
      '<div id="rf-tf-out" style="margin-top:14px">' + state.testResultHtml + '</div></div>';

    document.getElementById('rf-tf-file').onchange = function (e) {
      if (e.target.files[0]) {
        state.testPreview = URL.createObjectURL(e.target.files[0]);
        state.testResultHtml = '';
        renderTestFace(body);
      }
    };
    document.getElementById('rf-tf-run').onclick = async function () {
      var f = document.getElementById('rf-tf-file').files[0];
      if (!f) { toast('Choose a photo first', false); return; }
      var fd = new FormData();
      fd.append('image', f);
      fd.append('threshold', document.getElementById('rf-tf-thr').value);
      document.getElementById('rf-tf-out').innerHTML = '⏳ Running face detection and vector matching…';
      var r = await api('/admin/api/test-recognition', { method: 'POST', body: fd });
      if (!r.ok) { toast(r.data.reason || 'Test failed', false); return; }
      var dets = r.data.matches || r.data.detections || [];
      state.testResultHtml = dets.length ? '<div class="rf-grid">' + dets.map(function (x, i) {
        var ok = x.identity && x.identity !== 'Unknown';
        return '<div class="rf-card rf-card-col"><h3>Face ' + (i + 1) + ' <span class="rf-badge ' + (ok ? 'rf-badge-ok' : 'rf-badge-warn') + '">' + (ok ? 'MATCHED' : 'UNKNOWN') + '</span></h3>' +
          '<div><b>Identity:</b> ' + esc(x.identity || 'Unknown') + '</div>' +
          '<div><b>Confidence:</b> ' + Number(x.confidence || 0).toFixed(3) + '</div></div>';
      }).join('') + '</div>' : '<div class="rf-empty">No face detected in this image.</div>';
      document.getElementById('rf-tf-out').innerHTML = state.testResultHtml;
    };
  }

  // ─── Mount Button Directly Under MCP Settings in LibreChat Rail ───────────
  function createAdminButton() {
    var btn = document.createElement('button');
    btn.type = 'button';
    btn.id = BTN_ID;
    btn.setAttribute('data-testid', 'nav-panel-rojakface-admin');
    btn.setAttribute('aria-label', 'Admin Portal');
    btn.setAttribute('aria-pressed', state.open ? 'true' : 'false');
    btn.setAttribute('title', 'Admin Portal');
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
      tip.style.top = Math.round(rect.top + rect.height / 2 - 14) + 'px';
      tip.style.left = Math.round(rect.right + 10) + 'px';
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

  window.addEventListener('resize', function () { if (state.open) positionStage(); });

  var domObserver = new MutationObserver(mountAdminButton);
  domObserver.observe(document.documentElement, { childList: true, subtree: true });

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', mountAdminButton);
  else mountAdminButton();

  if (window.location.hash === '#rojakface-admin') {
    setTimeout(openAdmin, 450);
  }
})();
