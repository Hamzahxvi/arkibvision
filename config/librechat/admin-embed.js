(function () {
  'use strict';

  var BTN_ID = 'rf-admin-rail-btn';
  var GUEST_BTN_ID = 'rf-guest-rail-btn';
  var STAGE_ID = 'rf-admin-stage';
  var API_BASE = 'http://' + (window.location.hostname || 'localhost') + ':8005';
  var GUEST_TABS = ['people', 'locations', 'documents', 'test'];

  var TRASH_SVG = '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/><line x1="10" x2="10" y1="11" y2="17"/><line x1="14" x2="14" y1="11" y2="17"/></svg>';
  var TRASH_LG_SVG = '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/><line x1="10" x2="10" y1="11" y2="17"/><line x1="14" x2="14" y1="11" y2="17"/></svg>';
  var DOTS_SVG = '<svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="5" r="1.2" fill="currentColor"/><circle cx="12" cy="12" r="1.2" fill="currentColor"/><circle cx="12" cy="19" r="1.2" fill="currentColor"/></svg>';
  var EDIT_SVG = '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"/></svg>';
  var EYE_SVG = '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0"/><circle cx="12" cy="12" r="3"/></svg>';

  var state = {
    open: false,
    mode: 'admin',
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
    categories: [],
    docSort: 'newest',
    docCategoryFilter: 'all',
    docSourceFilter: 'all',
    preferencesData: null,
    auditData: null,
    auditChangeFilter: 'all',
    auditAiExplanation: '',
    auditAiSource: '',
    behaviorData: null,
    diskScanResult: null,
    sqlInspectResult: null,
    health: null,
    integrity: null,
    labHtml: '',
    labFile: null,
    labFileName: '',
    labPreviewUrl: '',
    labMode: 'auto',
    labEnhancedUrl: '',
    labEnhancedMeta: null,
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

  // ─── Shared ArkibVision Structured Input Validation & AI Helpers ──────────
  function setFieldError(inputEl, message) {
    if (!inputEl) return;
    var field = inputEl.closest('.arkib-field, .rf-field, .ac-field');
    if (!field) {
      field = inputEl.parentElement;
    }
    if (!field) return;
    field.classList.add('is-invalid');
    inputEl.setAttribute('aria-invalid', 'true');
    var errEl = field.querySelector(':scope > .arkib-field-error');
    if (!errEl) {
      errEl = document.createElement('div');
      errEl.className = 'arkib-field-error';
      errEl.setAttribute('role', 'alert');
      if (inputEl.nextSibling) field.insertBefore(errEl, inputEl.nextSibling);
      else field.appendChild(errEl);
    }
    errEl.textContent = String(message || 'This field is invalid.');
    if (!inputEl.__arkibErrBound) {
      inputEl.__arkibErrBound = true;
      var clearSelf = function () { clearFieldError(inputEl); };
      inputEl.addEventListener('input', clearSelf);
      inputEl.addEventListener('change', clearSelf);
    }
  }

  function clearFieldError(inputEl) {
    if (!inputEl) return;
    var field = inputEl.closest('.arkib-field, .rf-field, .ac-field') || inputEl.parentElement;
    if (field) {
      field.classList.remove('is-invalid');
      var errEl = field.querySelector(':scope > .arkib-field-error');
      if (errEl) errEl.remove();
    }
    inputEl.removeAttribute('aria-invalid');
  }

  function clearFormErrors(containerEl) {
    if (!containerEl) return;
    containerEl.querySelectorAll('.is-invalid').forEach(function (f) {
      f.classList.remove('is-invalid');
    });
    containerEl.querySelectorAll('.arkib-field-error').forEach(function (e) {
      e.remove();
    });
    containerEl.querySelectorAll('[aria-invalid="true"]').forEach(function (el) {
      el.removeAttribute('aria-invalid');
    });
  }

  function isMissingDescription(val) {
    var s = String(val == null ? '' : val).trim().toLowerCase();
    return !s || s === 'unknown' || s === 'n/a' || s === 'none' || s === '-' || s === 'null' || s === 'no description' || s === 'no description yet.';
  }

  window.ArkibForm = {
    setFieldError: setFieldError,
    clearFieldError: clearFieldError,
    clearFormErrors: clearFormErrors,
    isMissingDescription: isMissingDescription
  };

  function openViewer(src) {
    var v = document.createElement('div');
    v.className = 'rf-viewer';
    v.innerHTML = '<img src="' + esc(src) + '" alt="Preview"/>';
    v.addEventListener('click', function () { v.remove(); });
    document.body.appendChild(v);
  }

  var _openMenuAnchor = null;
  function closeMenus() {
    _openMenuAnchor = null;
    document.querySelectorAll('.rf-menu-pop').forEach(function (m) { m.remove(); });
  }

  function openCardActionMenu(anchorBtn, items) {
    if (_openMenuAnchor === anchorBtn) {
      closeMenus();
      return;
    }
    closeMenus();
    _openMenuAnchor = anchorBtn;
    var rect = anchorBtn.getBoundingClientRect();
    var pop = document.createElement('div');
    pop.className = 'rf-menu-pop';
    pop.setAttribute('role', 'menu');
    pop.style.minWidth = '188px';

    items.forEach(function (item) {
      if (!item) return;
      if (item.divider) {
        var hr = document.createElement('div');
        hr.className = 'rf-menu-sep';
        pop.appendChild(hr);
        return;
      }
      if (item.href) {
        var a = document.createElement('a');
        a.className = 'rf-menu-item' + (item.danger ? ' danger' : '');
        a.href = item.href;
        if (item.download) a.setAttribute('download', item.download);
        a.innerHTML = (item.icon || '') + '<span>' + esc(item.label) + '</span>';
        a.onclick = function () {
          if (item.onClick) item.onClick();
          closeMenus();
        };
        pop.appendChild(a);
      } else {
        var b = document.createElement('button');
        b.type = 'button';
        b.className = 'rf-menu-item' + (item.danger ? ' danger' : '');
        b.innerHTML = (item.icon || '') + '<span>' + esc(item.label) + '</span>';
        b.onclick = function (e) {
          e.stopPropagation();
          closeMenus();
          if (item.onClick) item.onClick();
        };
        pop.appendChild(b);
      }
    });

    document.body.appendChild(pop);
    var popW = pop.offsetWidth || 190;
    var popH = pop.offsetHeight || 160;
    var top = rect.bottom + 6;
    if (top + popH > window.innerHeight - 10) {
      top = Math.max(8, rect.top - popH - 6);
    }
    var left = Math.min(window.innerWidth - popW - 10, Math.max(8, Math.round(rect.right - popW)));
    pop.style.top = Math.round(top) + 'px';
    pop.style.left = Math.round(left) + 'px';
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

  function isGuest() {
    return state.mode === 'guest';
  }

  function setButtonActive(active) {
    var adminBtn = document.getElementById(BTN_ID);
    if (adminBtn) {
      var aOn = !!(active && !isGuest());
      adminBtn.classList.toggle('rf-active', aOn);
      adminBtn.setAttribute('aria-pressed', aOn ? 'true' : 'false');
    }
    var guestBtn = document.getElementById(GUEST_BTN_ID);
    if (guestBtn) {
      var gOn = !!(active && isGuest());
      guestBtn.classList.toggle('rf-active', gOn);
      guestBtn.setAttribute('aria-pressed', gOn ? 'true' : 'false');
    }
  }

  function fmtTime(iso) {
    if (!iso) return '—';
    try {
      var d = new Date(iso);
      if (isNaN(d.getTime())) return String(iso).replace('T', ' ').slice(0, 19);
      var diffSec = Math.max(0, Math.floor((Date.now() - d.getTime()) / 1000));
      var rel = diffSec < 60 ? 'just now'
        : diffSec < 3600 ? Math.floor(diffSec / 60) + 'm ago'
        : diffSec < 86400 ? Math.floor(diffSec / 3600) + 'h ago'
        : Math.floor(diffSec / 86400) + 'd ago';
      var formatted = d.toLocaleString(undefined, {
        day: '2-digit',
        month: 'short',
        year: 'numeric',
        hour: '2-digit',
        minute: '2-digit'
      });
      return formatted + ' · ' + rel;
    } catch (e) {
      return String(iso).replace('T', ' ').slice(0, 19);
    }
  }

  var _searchTrackTimer = null;
  function trackSearchDebounced(entityType, queryText) {
    clearTimeout(_searchTrackTimer);
    var q = String(queryText || '').trim();
    if ((!state.authed && !isGuest()) || q.length < 3) return;
    var endpoint = isGuest() ? '/guest/api/analytics/track' : '/admin/api/analytics/track';
    var src = isGuest() ? 'guest_search' : 'admin_search';
    _searchTrackTimer = setTimeout(function () {
      api(endpoint, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          source: src,
          query_text: q,
          entity_type: entityType || ''
        })
      }).catch(function () {});
    }, 1100);
  }

  function trackGuestView(entityType, slug, title, category) {
    if (!isGuest()) return;
    try {
      fetch(apiUrl('/guest/api/analytics/track'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          source: 'guest_view',
          entity_type: entityType || '',
          item_slug: slug || '',
          item_title: title || '',
          category: category || ''
        })
      }).catch(function () {});
    } catch (e) {}
  }

  function trackUserPref(evt) {
    if (isGuest() || !state.authed || !evt) return;
    try {
      api('/admin/api/preferences/track', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(Object.assign({
          timestamp: '',
          epoch_secs: 0,
          event_type: 'click_document',
          category: '',
          source: '',
          group_folder: '',
          year: '',
          tags: [],
          item_slug: '',
          item_title: ''
        }, evt))
      }).then(function () {
        api('/admin/api/preferences?ts=' + Date.now()).then(function (pr) {
          if (pr.ok && pr.data) state.preferencesData = pr.data;
        }).catch(function () {});
      }).catch(function () {});
    } catch (e) {}
  }

  var _tabFetchedAt = {};

  async function fetchHeavyTabData(tab, force) {
    if (!state.authed || isGuest()) return;
    var now = Date.now();
    if (!force && _tabFetchedAt[tab] && (now - _tabFetchedAt[tab]) < 8000) return;
    _tabFetchedAt[tab] = now;
    try {
      if (tab === 'audit') {
        var ar = await api('/admin/api/audit/dashboard?ts=' + now);
        if (ar.ok && ar.data) state.auditData = ar.data;
        render();
      } else if (tab === 'insights') {
        var br = await api('/admin/api/analytics/behavior?ts=' + now);
        if (br.ok && br.data) state.behaviorData = br.data;
        render();
      } else if (tab === 'system') {
        var hr = await api('/admin/api/system/health?ts=' + now);
        if (hr.ok) state.health = hr.data;
        render();
      } else if (tab === 'documents') {
        var pr = await api('/admin/api/preferences?ts=' + now);
        if (pr.ok && pr.data) state.preferencesData = pr.data;
        render();
      }
    } catch (e) {}
  }

  async function refreshAll(manual) {
    if (!state.authed) return;
    try {
      var ts = Date.now();
      var reqs = [
        api('/admin/api/requests?ts=' + ts),
        api('/admin/api/profiles?ts=' + ts),
        api('/admin/api/people-groups?ts=' + ts),
        api('/admin/api/locations?ts=' + ts),
        api('/admin/api/location-groups?ts=' + ts),
        api('/admin/api/documents?ts=' + ts),
        api('/admin/api/document-groups?ts=' + ts),
        api('/admin/api/categories?ts=' + ts)
      ];
      // Load heavy analytics/audit/system/preferences endpoints ONLY for the active tab (or explicit manual refresh)
      var wantHealth = manual || state.tab === 'system';
      var wantPrefs = manual || state.tab === 'documents';
      var wantAudit = manual || state.tab === 'audit';
      var wantBehavior = manual || state.tab === 'insights';
      if (wantHealth) reqs.push(api('/admin/api/system/health?ts=' + ts));
      if (wantPrefs) reqs.push(api('/admin/api/preferences?ts=' + ts));
      if (wantAudit) reqs.push(api('/admin/api/audit/dashboard?ts=' + ts));
      if (wantBehavior) reqs.push(api('/admin/api/analytics/behavior?ts=' + ts));

      var res = await Promise.all(reqs);
      if (res[0].ok && Array.isArray(res[0].data)) state.requests = res[0].data;
      if (res[1].ok && Array.isArray(res[1].data)) state.profiles = res[1].data;
      if (res[2].ok && Array.isArray(res[2].data)) state.peopleGroups = res[2].data;
      if (res[3].ok && Array.isArray(res[3].data)) state.locations = res[3].data;
      if (res[4].ok && Array.isArray(res[4].data)) state.locationGroups = res[4].data;
      if (res[5].ok && Array.isArray(res[5].data)) state.documents = res[5].data;
      if (res[6].ok && Array.isArray(res[6].data)) state.documentGroups = res[6].data;
      if (res[7].ok && Array.isArray(res[7].data)) state.categories = res[7].data;
      var idx = 8;
      if (wantHealth) {
        if (res[idx] && res[idx].ok) { state.health = res[idx].data; _tabFetchedAt.system = ts; }
        idx++;
      }
      if (wantPrefs) {
        if (res[idx] && res[idx].ok && res[idx].data) { state.preferencesData = res[idx].data; _tabFetchedAt.documents = ts; }
        idx++;
      }
      if (wantAudit) {
        if (res[idx] && res[idx].ok && res[idx].data) { state.auditData = res[idx].data; _tabFetchedAt.audit = ts; }
        idx++;
      }
      if (wantBehavior) {
        if (res[idx] && res[idx].ok && res[idx].data) { state.behaviorData = res[idx].data; _tabFetchedAt.insights = ts; }
      }
      render();
      if (manual) toast('Admin data refreshed');
    } catch (e) {}
  }

  async function refreshGuestAll(manual) {
    try {
      var res = await Promise.all([
        api('/guest/api/profiles?ts=' + Date.now()),
        api('/guest/api/people-groups?ts=' + Date.now()),
        api('/guest/api/locations?ts=' + Date.now()),
        api('/guest/api/location-groups?ts=' + Date.now()),
        api('/guest/api/documents?ts=' + Date.now()),
        api('/guest/api/document-groups?ts=' + Date.now()),
        api('/guest/api/categories?ts=' + Date.now())
      ]);
      if (res[0].ok && Array.isArray(res[0].data)) state.profiles = res[0].data;
      if (res[1].ok && Array.isArray(res[1].data)) state.peopleGroups = res[1].data;
      if (res[2].ok && Array.isArray(res[2].data)) state.locations = res[2].data;
      if (res[3].ok && Array.isArray(res[3].data)) state.locationGroups = res[3].data;
      if (res[4].ok && Array.isArray(res[4].data)) state.documents = res[4].data;
      if (res[5].ok && Array.isArray(res[5].data)) state.documentGroups = res[5].data;
      if (res[6].ok && Array.isArray(res[6].data)) state.categories = res[6].data;
      state.requests = [];
      state.health = null;
      state.preferencesData = null;
      state.auditData = null;
      state.behaviorData = null;
      render();
      if (manual) toast('Archive data refreshed');
    } catch (e) {
      toast('Could not reach local ArkibVision service (port 8005).', false);
    }
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
    stage.setAttribute('aria-label', 'Admin Portal');
    state.mode = 'admin';
    state.open = true;
    positionStage();
    stage.style.display = 'block';
    setButtonActive(true);
    setTimeout(positionStage, 120);
    checkSessionAndLoad();
    if (!state.pollTimer) {
      state.pollTimer = setInterval(function () {
        if (!document.hidden && state.open && !isGuest() && state.authed && !document.querySelector('.rf-modal-backdrop')) {
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

  function openGuest() {
    var closeDrawerBtn = document.querySelector('[data-testid="close-sidebar-button"]');
    if (closeDrawerBtn) {
      try { closeDrawerBtn.click(); } catch (e) {}
    }
    var stage = ensureStage();
    stage.setAttribute('aria-label', 'Guest Portal (Read-Only)');
    state.mode = 'guest';
    state.open = true;
    if (GUEST_TABS.indexOf(state.tab) === -1) state.tab = 'people';
    if (state.docSort === 'for_you') state.docSort = 'newest';
    state.testUnrecognized = false;
    positionStage();
    stage.style.display = 'block';
    setButtonActive(true);
    setTimeout(positionStage, 120);
    render();
    refreshGuestAll(false);
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
    var guestMode = isGuest();
    if (!guestMode && !state.authed) {
      stage.innerHTML =
        '<div class="rf-login-wrap"><div class="rf-login-card">' +
        '<span class="rf-pill">🛡️ ARKIBVISION · ADMIN PORTAL</span>' +
        '<h2>Sign in to Admin Portal</h2>' +
        '<p>Manage approvals, enrolled people, archival locations, documents, file integrity, and system health right inside LibreChat.</p>' +
        '<form id="rf-login-form" novalidate>' +
        '<div class="rf-field arkib-field" style="margin-bottom:14px"><label class="arkib-label" for="rf-u">Username</label><input class="rf-input arkib-input" id="rf-u" placeholder="Enter username" autocomplete="username" required/></div>' +
        '<div class="rf-field arkib-field" style="margin-bottom:18px"><label class="arkib-label" for="rf-p">Password</label><input class="rf-input arkib-input" id="rf-p" type="password" placeholder="Enter password" autocomplete="current-password" required/></div>' +
        '<button type="submit" class="rf-btn rf-btn-primary" style="width:100%;padding:10px">Sign in</button>' +
        '</form>' +
        '<div class="rf-login-guest-sep"><span>or browse without signing in</span></div>' +
        '<button type="button" class="rf-btn rf-btn-guest" id="rf-login-guest-btn" style="width:100%;padding:10px;justify-content:center">👁️ Open Guest Portal (Read-Only)</button>' +
        '</div></div>';
      var guestJump = document.getElementById('rf-login-guest-btn');
      if (guestJump) guestJump.addEventListener('click', function () { openGuest(); });
      var form = document.getElementById('rf-login-form');
      form.addEventListener('submit', async function (ev) {
        ev.preventDefault();
        clearFormErrors(form);
        var uEl = document.getElementById('rf-u');
        var pEl = document.getElementById('rf-p');
        var uVal = uEl.value.trim();
        var pVal = pEl.value;
        var hasErr = false;
        if (!uVal) { setFieldError(uEl, 'Username is required.'); hasErr = true; }
        if (!pVal) { setFieldError(pEl, 'Password is required.'); hasErr = true; }
        if (hasErr) { (uVal ? pEl : uEl).focus(); return; }
        try {
          var r = await api('/admin/api/login', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
              username: uVal,
              password: pVal
            })
          });
          if (!r.ok) {
            setFieldError(pEl, (r.data && r.data.reason) || 'Invalid username or password.');
            pEl.focus();
            return;
          }
          state.authed = true;
          state.user = (r.data && r.data.user) || 'admin';
          render();
          await refreshAll(false);
          toast('Signed in to Admin Portal');
        } catch (e) {
          setFieldError(pEl, 'Could not reach local ArkibVision service (port 8005).');
        }
      });
      return;
    }

    if (guestMode && GUEST_TABS.indexOf(state.tab) === -1) {
      state.tab = 'people';
    }

    var pendingCnt = state.requests.filter(function (x) { return x.status === 'pending'; }).length;
    var auditSnap = state.auditData && state.auditData.snapshot;
    var auditAlertsCnt = auditSnap
      ? ((auditSnap.rule_alerts ? auditSnap.rule_alerts.length : 0) + (auditSnap.statistical_outliers ? auditSnap.statistical_outliers.length : 0))
      : 0;
    var tabs = guestMode ? [
      { id: 'people', label: '👥 People', count: state.profiles.length },
      { id: 'locations', label: '📍 Locations', count: state.locations.length },
      { id: 'documents', label: '📁 Documents', count: state.documents.length },
      { id: 'test', label: '🔍 Test Face' }
    ] : [
      { id: 'approvals', label: '📋 Approvals', count: pendingCnt },
      { id: 'people', label: '👥 People', count: state.profiles.length },
      { id: 'locations', label: '📍 Locations', count: state.locations.length },
      { id: 'documents', label: '📁 Documents', count: state.documents.length },
      { id: 'insights', label: '📈 User Insights' },
      { id: 'audit', label: '🛡️ Audit & Changes', count: auditAlertsCnt > 0 ? auditAlertsCnt : undefined },
      { id: 'system', label: '📊 System & Lab' },
      { id: 'test', label: '🔍 Test Face' }
    ];

    var brandHtml = guestMode
      ? '<div class="rf-brand"><span>👁️ Guest Portal</span><span class="rf-pill rf-pill-guest">READ-ONLY</span></div>'
      : '<div class="rf-brand"><span>🛡️ Admin Portal</span><span class="rf-pill">LOCAL AI</span></div>';

    var actionsHtml = guestMode
      ? '<div class="rf-header-actions">' +
        '<button type="button" class="rf-btn rf-btn-sm" id="rf-refresh-btn">↻ Refresh</button>' +
        '<button type="button" class="rf-btn rf-btn-sm" id="rf-guest-close-btn">✕ Close</button>' +
        '</div>'
      : '<div class="rf-header-actions">' +
        '<button type="button" class="rf-btn rf-btn-sm" id="rf-refresh-btn">↻ Refresh</button>' +
        '<button type="button" class="rf-btn rf-btn-sm" id="rf-logout-btn">↪ Log out</button>' +
        '</div>';

    var guestBannerHtml = guestMode
      ? '<div class="rf-guest-banner"><span>👁️ <b>Guest Read-Only View</b> — Browse enrolled people, archival locations, digitized documents, and test face recognition. Editing, creating, deleting, and enrolling are restricted to Administrators.</span></div>'
      : '';

    stage.innerHTML =
      '<div class="rf-shell' + (guestMode ? ' rf-shell-guest' : '') + '">' +
      '<header class="rf-header">' +
      brandHtml +
      '<nav class="rf-tabs">' +
      tabs.map(function (t) {
        var cntHtml = typeof t.count === 'number'
          ? '<span class="rf-tab-count"' + (t.id === 'approvals' ? ' id="rf-pending-badge"' : '') + '>' + t.count + '</span>'
          : '';
        return '<button type="button" class="rf-tab' + (state.tab === t.id ? ' active' : '') + '" data-rf-tab="' + t.id + '">' + t.label + cntHtml + '</button>';
      }).join('') +
      '</nav>' +
      actionsHtml +
      '</header>' +
      guestBannerHtml +
      '<main class="rf-main" id="rf-main-body"></main>' +
      '</div>';

    stage.querySelectorAll('[data-rf-tab]').forEach(function (b) {
      b.addEventListener('click', function () {
        var nextTab = b.getAttribute('data-rf-tab');
        if (guestMode && GUEST_TABS.indexOf(nextTab) === -1) return;
        state.tab = nextTab;
        state.search = '';
        render();
        if (!guestMode && (nextTab === 'audit' || nextTab === 'insights' || nextTab === 'system' || nextTab === 'documents')) {
          fetchHeavyTabData(nextTab, false);
        }
      });
    });
    document.getElementById('rf-refresh-btn').addEventListener('click', function () {
      if (guestMode) refreshGuestAll(true);
      else refreshAll(true);
    });
    var logoutBtn = document.getElementById('rf-logout-btn');
    if (logoutBtn) {
      logoutBtn.addEventListener('click', async function () {
        await api('/admin/api/logout', { method: 'POST' });
        state.authed = false;
        render();
        toast('Signed out');
      });
    }
    var guestCloseBtn = document.getElementById('rf-guest-close-btn');
    if (guestCloseBtn) {
      guestCloseBtn.addEventListener('click', closeAdmin);
    }

    renderBody();
  }

  function renderBody() {
    var body = document.getElementById('rf-main-body');
    if (!body) return;
    if (isGuest()) {
      if (state.tab === 'people') renderPeople(body);
      else if (state.tab === 'locations') renderLocations(body);
      else if (state.tab === 'documents') renderDocuments(body);
      else if (state.tab === 'test') renderTestFace(body);
      else {
        state.tab = 'people';
        renderPeople(body);
      }
      return;
    }
    if (state.tab === 'approvals') renderApprovals(body);
    else if (state.tab === 'people') renderPeople(body);
    else if (state.tab === 'locations') renderLocations(body);
    else if (state.tab === 'documents') renderDocuments(body);
    else if (state.tab === 'insights') renderInsights(body);
    else if (state.tab === 'audit') renderAudit(body);
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

  function openGuestPersonModal(p) {
    if (!p) return;
    var guestMode = isGuest();
    if (guestMode) trackGuestView('profile', p.slug, p.identity, p.group || '');
    var imgs = Array.isArray(p.images) ? p.images : [];
    var wrap = document.createElement('div');
    wrap.className = 'rf-modal-backdrop';
    wrap.innerHTML =
      '<div class="rf-modal" style="width:min(680px,100%)">' +
      '<div class="rf-modal-head"><div><span class="rf-pill ' + (guestMode ? 'rf-pill-guest' : '') + '" style="margin-bottom:6px;display:inline-flex">' + (guestMode ? '👁️ READ-ONLY PROFILE' : '👁️ PROFILE DETAILS') + '</span><h2>👤 ' + esc(p.identity) + '</h2></div>' +
      '<div style="display:flex;gap:8px">' +
      (guestMode ? '' : '<button type="button" class="rf-btn rf-btn-primary rf-btn-sm" id="rf-gp-edit">' + EDIT_SVG + '<span>Edit</span></button>') +
      '<button type="button" class="rf-btn rf-btn-sm" id="rf-gp-close">✕ Close</button></div></div>' +
      '<div class="rf-guest-kv-grid">' +
      '<div class="rf-guest-kv"><span>Group Folder</span><strong>' + esc(p.group || 'Unassigned') + '</strong></div>' +
      '<div class="rf-guest-kv"><span>Biometric Vectors</span><strong>' + Number(p.vector_count || 0) + ' vector(s) · ' + imgs.length + ' photo(s)</strong></div>' +
      (p.address ? '<div class="rf-guest-kv rf-guest-kv-full"><span>Address / Affiliation</span><strong>' + esc(p.address) + '</strong></div>' : '') +
      '<div class="rf-guest-kv rf-guest-kv-full"><span>Notes &amp; Biography</span><p style="margin:4px 0 0;white-space:pre-wrap;line-height:1.55">' + esc(p.notes || p.description || 'No additional notes recorded.') + '</p></div>' +
      '</div>' +
      '<h3 style="margin:16px 0 10px;font-size:.92rem">Reference Photos (' + imgs.length + ')</h3>' +
      (imgs.length
        ? '<div class="rf-guest-gallery">' + imgs.map(function (im) {
            return '<figure class="rf-guest-photo" data-rf-zoom="' + esc(apiUrl(im.url)) + '">' +
              '<img src="' + esc(apiUrl(im.url)) + '" alt="' + esc(im.title || p.identity) + '"/>' +
              (im.title || im.description ? '<figcaption>' + esc(im.title || im.description) + '</figcaption>' : '') +
              '</figure>';
          }).join('') + '</div>'
        : '<div class="rf-empty">No reference photos available.</div>') +
      '</div>';
    document.body.appendChild(wrap);
    wrap.onclick = function (e) { if (e.target === wrap) wrap.remove(); };
    wrap.querySelector('#rf-gp-close').onclick = function () { wrap.remove(); };
    var gpEdit = wrap.querySelector('#rf-gp-edit');
    if (gpEdit) gpEdit.onclick = function () { wrap.remove(); openPersonModal(p); };
    wrap.querySelectorAll('[data-rf-zoom]').forEach(function (el) {
      el.onclick = function () { openViewer(el.getAttribute('data-rf-zoom')); };
    });
  }

  function renderPeople(body) {
    closeMenus();
    var guestMode = isGuest();
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

    var folderDropdownHtml =
      '<div class="rf-folder-dd-wrap">' +
      '<label class="rf-dd-label" for="rf-ppl-grp-sel">📁 Group Folder</label>' +
      '<select class="rf-select rf-folder-select" id="rf-ppl-grp-sel">' +
      '<option value="all"' + (state.selectedGroup === 'all' ? ' selected' : '') + '>📂 All Folders (' + state.profiles.length + ')</option>' +
      state.peopleGroups.map(function (g) {
        var gs = String(g.slug || '').toLowerCase();
        var cnt = state.profiles.filter(function (p) { return personGroupSlug(p) === gs; }).length;
        return '<option value="' + esc(gs) + '"' + (state.selectedGroup === gs ? ' selected' : '') + '>📁 ' + esc(g.name) + ' (' + cnt + ')</option>';
      }).join('') +
      '<option value="unassigned"' + (state.selectedGroup === 'unassigned' ? ' selected' : '') + '>📂 Unassigned (' + unassignedCnt + ')</option>' +
      '</select>' +
      (!guestMode && activeGroupObj ? '<button type="button" class="rf-btn rf-btn-sm" id="rf-ppl-grp-manage" title="Rename or delete selected folder">' + DOTS_SVG + '<span>Manage Folder</span></button>' : '') +
      '</div>';

    var groupOpts = function (curSlug) {
      return '<option value=""' + (!curSlug ? ' selected' : '') + '>📂 Unassigned</option>' +
        state.peopleGroups.map(function (g) {
          var gs = String(g.slug || '').toLowerCase();
          return '<option value="' + esc(gs) + '"' + (curSlug === gs ? ' selected' : '') + '>📁 ' + esc(g.name) + '</option>';
        }).join('');
    };

    body.innerHTML =
      '<div class="rf-section-head"><div><h2>👥 Enrolled People' + (activeGroupObj ? ' — 📁 ' + esc(activeGroupObj.name) : state.selectedGroup === 'unassigned' ? ' — Unassigned' : '') + '</h2>' +
      '<p>' + esc(activeGroupObj && activeGroupObj.description ? activeGroupObj.description : (guestMode ? 'Browse enrolled identities by group folder or keyword (read-only).' : 'Organize enrolled people into maintainable group folders. Select a folder from the dropdown to filter or manage it.')) + '</p></div>' +
      (guestMode ? '' : '<div style="display:flex;gap:8px;flex-wrap:wrap">' +
      '<button type="button" class="rf-btn" id="rf-add-group-btn">📁＋ New Group Folder</button>' +
      '<button type="button" class="rf-btn rf-btn-primary" id="rf-add-person-btn">＋ Add Person</button>' +
      '</div>') + '</div>' +
      '<div class="rf-toolbar rf-toolbar-wrap">' +
      folderDropdownHtml +
      '<input class="rf-input arkib-input arkib-input--underline" id="rf-ppl-search" style="flex:1;min-width:220px" type="search" placeholder="Search enrolled people in this folder…" value="' + esc(state.search) + '"/>' +
      '</div>' +
      '<div class="rf-grid">' +
      (list.length ? list.map(function (p) {
        var img = p.images && p.images[0] ? '<img class="rf-thumb" data-rf-zoom="' + esc(apiUrl(p.images[0].url)) + '" style="cursor:pointer" src="' + esc(apiUrl(p.images[0].url)) + '" alt="' + esc(p.identity) + '"/>' : '<div class="rf-thumb">👤</div>';
        var curGs = personGroupSlug(p);
        var revCnt = Number(p.revision_count || 0);
        var actionsHtml = guestMode
          ? '<div class="rf-actions">' +
            '<span class="rf-folder-readonly" title="Group folder (read-only)">' + (p.group ? '📁 ' + esc(p.group) : '📂 Unassigned') + '</span>' +
            '<button type="button" class="rf-btn rf-btn-sm" data-rf-view-p="' + esc(p.slug) + '">' + EYE_SVG + '<span>View Details</span></button>' +
            '</div>'
          : '<div class="rf-actions rf-actions-compact">' +
            '<select class="rf-move-sel" data-rf-move-p="' + esc(p.slug) + '" title="Change group folder">' + groupOpts(curGs) + '</select>' +
            '<button type="button" class="rf-btn rf-card-menu-btn" data-rf-menu-p="' + esc(p.slug) + '" title="Actions (View, Edit, Delete)" aria-label="Person actions">' + DOTS_SVG + '</button>' +
            '</div>';
        return '<article class="rf-card">' + img + '<div class="rf-card-body">' +
          '<h3 title="' + esc(p.identity) + '">' + esc(p.identity) + '</h3>' +
          '<p class="rf-desc">' + esc(p.notes || p.address || p.description || 'No notes') + '</p>' +
          '<div class="rf-meta">' + Number(p.vector_count || 0) + ' vector(s) · ' + (p.images ? p.images.length : 0) + ' photo(s)' +
          (!guestMode && revCnt > 0 ? ' · <span class="rf-badge rf-badge-info">🕒 ' + revCnt + ' change' + (revCnt === 1 ? '' : 's') + '</span>' : '') + '</div>' +
          actionsHtml + '</div></article>';
      }).join('') : '<div class="rf-empty">' + (guestMode ? 'No enrolled people match your filter.' : 'No enrolled people in this folder yet. Move someone here using the folder dropdown or click "＋ Add Person".') + '</div>') +
      '</div>';

    var addGrpBtn = document.getElementById('rf-add-group-btn');
    if (addGrpBtn) addGrpBtn.onclick = function () { openGroupModal('people', null); };
    var addPplBtn = document.getElementById('rf-add-person-btn');
    if (addPplBtn) addPplBtn.onclick = function () { openPersonModal(null); };
    body.querySelectorAll('[data-rf-zoom]').forEach(function (imgEl) {
      imgEl.onclick = function () { openViewer(imgEl.getAttribute('data-rf-zoom')); };
    });
    body.querySelectorAll('[data-rf-view-p]').forEach(function (b) {
      b.onclick = function () {
        var p = state.profiles.find(function (x) { return x.slug === b.getAttribute('data-rf-view-p'); });
        if (p) openGuestPersonModal(p);
      };
    });
    document.getElementById('rf-ppl-grp-sel').onchange = function (e) {
      state.selectedGroup = e.target.value;
      if (state.selectedGroup !== 'all' && state.selectedGroup !== 'unassigned') {
        var grpObj = state.peopleGroups.find(function (x) { return String(x.slug || '').toLowerCase() === state.selectedGroup; });
        trackUserPref({ event_type: 'filter_folder', group_folder: grpObj ? grpObj.name : state.selectedGroup });
      }
      renderPeople(body);
    };
    var managePplGrp = document.getElementById('rf-ppl-grp-manage');
    if (managePplGrp && activeGroupObj) {
      managePplGrp.onclick = function (ev) {
        ev.stopPropagation();
        var gs = String(activeGroupObj.slug || '').toLowerCase();
        var memberCnt = state.profiles.filter(function (p) { return personGroupSlug(p) === gs; }).length;
        openFolderMenu(managePplGrp, {
          onRename: function () { openGroupModal('people', activeGroupObj); },
          onDelete: function () {
            openConfirmModal({
              title: 'Delete Group Folder "' + activeGroupObj.name + '"?',
              message: 'This removes the "' + activeGroupObj.name + '" folder. The ' + memberCnt + ' enrolled ' + (memberCnt === 1 ? 'person' : 'people') + ' inside will NOT be deleted — they will move safely to Unassigned.',
              confirmLabel: 'Delete Folder',
              onConfirm: async function () {
                var r = await api('/admin/api/people-groups/' + encodeURIComponent(activeGroupObj.slug), { method: 'DELETE' });
                if (r.ok) {
                  state.selectedGroup = 'all';
                  toast('Deleted folder "' + activeGroupObj.name + '"');
                  await refreshAll(false);
                } else toast((r.data && r.data.reason) || 'Delete failed', false);
              }
            });
          }
        });
      };
    }
    document.getElementById('rf-ppl-search').oninput = function (e) {
      var pos = e.target.selectionStart;
      state.search = e.target.value;
      trackSearchDebounced('profile', state.search);
      renderPeople(body);
      restoreFocus('rf-ppl-search', pos);
    };
    body.querySelectorAll('[data-rf-rev-p]').forEach(function (b) {
      b.onclick = function () {
        var slug = b.getAttribute('data-rf-rev-p');
        var p = state.profiles.find(function (x) { return x.slug === slug; });
        openRevisionsModal('profile', slug, p ? p.identity : slug);
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
    body.querySelectorAll('[data-rf-menu-p]').forEach(function (btn) {
      btn.onclick = function (ev) {
        ev.stopPropagation();
        var p = state.profiles.find(function (x) { return x.slug === btn.getAttribute('data-rf-menu-p'); });
        if (!p) return;
        var revCnt = Number(p.revision_count || 0);
        var vc = Number(p.vector_count || 0);
        var ic = p.images ? p.images.length : 0;
        var items = [
          { label: 'View Details', icon: EYE_SVG, onClick: function () { openGuestPersonModal(p); } },
          { label: 'Edit Person', icon: EDIT_SVG, onClick: function () { openPersonModal(p); } }
        ];
        if (revCnt > 0) {
          items.push({ label: 'View Changes (' + revCnt + ')', icon: '<span>🕒</span>', onClick: function () { openRevisionsModal('profile', p.slug, p.identity); } });
        }
        items.push({ divider: true });
        items.push({
          label: 'Delete Person',
          icon: TRASH_SVG,
          danger: true,
          onClick: function () {
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
          }
        });
        openCardActionMenu(btn, items);
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
      '<form id="rf-g-form" novalidate><div class="rf-form-grid">' +
      '<div class="rf-field arkib-field full"><label class="arkib-label" for="rf-g-name">Group Folder Name</label><input class="rf-input arkib-input" id="rf-g-name" required placeholder="' + placeholder + '" value="' + esc(group ? group.name || '' : '') + '"/></div>' +
      '<div class="rf-field arkib-field full"><label class="arkib-label" for="rf-g-desc">Description</label><textarea class="rf-textarea arkib-input" id="rf-g-desc" placeholder="Optional description of what belongs in this folder…">' + esc(group ? group.description || '' : '') + '</textarea></div>' +
      '</div><div class="rf-actions" style="justify-content:flex-end;margin-top:16px">' +
      '<button type="button" class="rf-btn" id="rf-g-cancel">Cancel</button>' +
      '<button type="submit" class="rf-btn rf-btn-primary">' + (group ? 'Save Rename' : 'Create Folder') + '</button>' +
      '</div></form></div>';
    document.body.appendChild(wrap);
    wrap.onclick = function (e) { if (e.target === wrap) wrap.remove(); };
    document.getElementById('rf-g-cancel').onclick = function () { wrap.remove(); };
    document.getElementById('rf-g-form').onsubmit = async function (ev) {
      ev.preventDefault();
      var gForm = ev.target;
      clearFormErrors(gForm);
      var nameEl = document.getElementById('rf-g-name');
      var name = nameEl.value.trim();
      var description = document.getElementById('rf-g-desc').value.trim();
      if (!name) {
        setFieldError(nameEl, 'Group folder name is required.');
        nameEl.focus();
        return;
      }
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
      } else {
        setFieldError(nameEl, (r.data && r.data.reason) || 'Could not save group folder.');
        nameEl.focus();
      }
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
      '<form id="rf-p-form" novalidate><div class="rf-form-grid">' +
      '<div class="rf-field arkib-field"><label class="arkib-label" for="rf-p-name">Name</label><input class="rf-input arkib-input" id="rf-p-name" placeholder="Full name" required value="' + esc(profile ? profile.identity : '') + '"/></div>' +
      '<div class="rf-field arkib-field"><label class="arkib-label" for="rf-p-group">Group Folder</label><select class="rf-select arkib-input" id="rf-p-group">' + grpSelectHtml + '</select></div>' +
      (!profile ? '<div class="rf-field arkib-field full"><label class="arkib-label" for="rf-p-img">Face Photo (required)</label><input class="rf-input arkib-input" id="rf-p-img" type="file" accept=".jpg,.jpeg,.png,.webp,.bmp" required/></div>' : '') +
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
            '<div style="display:flex;justify-content:space-between;align-items:center;flex-wrap:wrap;gap:8px;margin-bottom:10px"><b>Photo: ' + esc(fn) + '</b>' +
            '<div style="display:flex;gap:6px;flex-wrap:wrap">' +
            '<button type="button" class="rf-btn rf-btn-sm" id="rf-pm-ai-btn" title="Generate AI description from image">✨ AI Suggest</button>' +
            '<button type="button" class="rf-btn rf-btn-sm" id="rf-pm-zoom">🔍 Enlarge</button>' +
            '<button type="button" class="rf-btn rf-btn-ok rf-btn-sm" id="rf-pm-save">Save photo details</button></div></div>' +
            '<div class="rf-form-grid">' +
            '<div class="rf-field arkib-field full"><label class="arkib-label" for="rf-pm-desc">Description</label>' +
            '<input class="rf-input arkib-input" id="rf-pm-desc" placeholder="Describe this photo (or use AI Suggest)…" value="' + esc(im.description || '') + '"/>' +
            '<div id="rf-pm-ai-slot"></div></div>' +
            '<div class="rf-field arkib-field"><label class="arkib-label" for="rf-pm-by">Photograph by</label><input class="rf-input arkib-input" id="rf-pm-by" value="' + esc(im.photograph_by || '') + '"/></div>' +
            '<div class="rf-field arkib-field"><label class="arkib-label" for="rf-pm-loc">Location</label><input class="rf-input arkib-input" id="rf-pm-loc" value="' + esc(im.location || '') + '"/></div>' +
            '<div class="rf-field arkib-field"><label class="arkib-label" for="rf-pm-date">Date</label><input class="rf-input arkib-input" id="rf-pm-date" type="date" value="' + esc(im.date || '') + '"/></div>' +
            '<div class="rf-field arkib-field"><label class="arkib-label" for="rf-pm-time">Time</label><input class="rf-input arkib-input" id="rf-pm-time" type="time" value="' + esc(im.time || '') + '"/></div>' +
            '</div>';

          async function fetchAiPhotoDesc() {
            var slot = document.getElementById('rf-pm-ai-slot');
            var aiBtn = document.getElementById('rf-pm-ai-btn');
            if (!slot) return;
            if (aiBtn) aiBtn.disabled = true;
            slot.innerHTML =
              '<div class="rf-ai-desc-box">' +
              '<div class="rf-ai-desc-head"><span>✨ AI Description Suggestion</span><span class="rf-badge rf-badge-info">Analyzing…</span></div>' +
              '<div class="rf-ai-desc-text" style="opacity:.8">⏳ Inspecting photo with local Vision AI…</div>' +
              '</div>';
            try {
              var res = await api('/admin/api/suggest-image-description', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                  entity_type: 'profile',
                  slug: cur.slug,
                  filename: fn,
                  context_label: cur.identity || cur.slug
                })
              });
              // Guard against user clicking another photo while request was in flight
              if (state.selectedPhoto !== fn) return;
              slot = document.getElementById('rf-pm-ai-slot');
              if (!slot) return;
              var sug = res.ok && res.data && res.data.suggested_description ? String(res.data.suggested_description).trim() : '';
              if (!sug) {
                slot.innerHTML = '';
                return;
              }
              var srcBadge = res.data.source === 'ollama_vision' ? 'Local Vision AI' : 'Metadata Fallback';
              slot.innerHTML =
                '<div class="rf-ai-desc-box">' +
                '<div class="rf-ai-desc-head"><span>✨ AI Suggested Description</span><span class="rf-badge rf-badge-ok">' + esc(srcBadge) + '</span></div>' +
                '<div class="rf-ai-desc-text">' + esc(sug) + '</div>' +
                '<div class="rf-ai-desc-actions">' +
                '<button type="button" class="rf-btn rf-btn-primary rf-btn-sm" id="rf-pm-ai-apply">✓ Apply Description</button>' +
                '<button type="button" class="rf-btn rf-btn-sm" id="rf-pm-ai-retry">↻ Regenerate</button>' +
                '<button type="button" class="rf-btn rf-btn-sm" id="rf-pm-ai-dismiss">✕ Dismiss</button>' +
                '</div></div>';
              document.getElementById('rf-pm-ai-apply').onclick = function () {
                var descInput = document.getElementById('rf-pm-desc');
                if (descInput) {
                  descInput.value = sug;
                  clearFieldError(descInput);
                  descInput.focus();
                }
                slot.innerHTML = '';
                toast('AI description applied — click "Save photo details" to persist');
              };
              document.getElementById('rf-pm-ai-retry').onclick = fetchAiPhotoDesc;
              document.getElementById('rf-pm-ai-dismiss').onclick = function () { slot.innerHTML = ''; };
            } catch (err) {
              if (slot) slot.innerHTML = '';
            } finally {
              var b = document.getElementById('rf-pm-ai-btn');
              if (b) b.disabled = false;
            }
          }

          document.getElementById('rf-pm-ai-btn').onclick = fetchAiPhotoDesc;
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

          // Auto-suggest description when the clicked photo has no description (or "Unknown")
          if (isMissingDescription(im.description)) {
            fetchAiPhotoDesc();
          }
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
      var pForm = ev.target;
      clearFormErrors(pForm);
      var nameEl = document.getElementById('rf-p-name');
      var imgEl = document.getElementById('rf-p-img');
      var hasErr = false;
      if (!nameEl.value.trim()) {
        setFieldError(nameEl, 'Person name is required.');
        hasErr = true;
      }
      if (!profile && imgEl && (!imgEl.files || !imgEl.files[0])) {
        setFieldError(imgEl, 'Please choose a face photo to enroll.');
        hasErr = true;
      }
      if (hasErr) {
        (nameEl.value.trim() ? imgEl : nameEl).focus();
        return;
      }
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

  function openGuestLocationModal(l) {
    if (!l) return;
    var guestMode = isGuest();
    if (guestMode) trackGuestView('location', l.slug, l.name, l.category || l.group || '');
    var imgs = Array.isArray(l.images) ? l.images : [];
    var wrap = document.createElement('div');
    wrap.className = 'rf-modal-backdrop';
    wrap.innerHTML =
      '<div class="rf-modal" style="width:min(700px,100%)">' +
      '<div class="rf-modal-head"><div><span class="rf-pill ' + (guestMode ? 'rf-pill-guest' : '') + '" style="margin-bottom:6px;display:inline-flex">' + (guestMode ? '👁️ READ-ONLY LOCATION' : '👁️ LOCATION DETAILS') + '</span><h2>📍 ' + esc(l.name) + '</h2></div>' +
      '<div style="display:flex;gap:8px">' +
      (guestMode ? '' : '<button type="button" class="rf-btn rf-btn-primary rf-btn-sm" id="rf-gl-edit">' + EDIT_SVG + '<span>Edit</span></button>') +
      '<button type="button" class="rf-btn rf-btn-sm" id="rf-gl-close">✕ Close</button></div></div>' +
      '<div class="rf-guest-kv-grid">' +
      '<div class="rf-guest-kv"><span>Group Folder</span><strong>' + esc(l.group || 'Unassigned') + '</strong></div>' +
      '<div class="rf-guest-kv"><span>Category</span><strong>' + esc(l.category || 'Archival Location') + '</strong></div>' +
      (l.parent_institution ? '<div class="rf-guest-kv"><span>Institution</span><strong>' + esc(l.parent_institution) + '</strong></div>' : '') +
      ((l.building_or_wing || l.floor_or_room) ? '<div class="rf-guest-kv"><span>Building / Room</span><strong>' + esc([l.building_or_wing, l.floor_or_room].filter(Boolean).join(' · ')) + '</strong></div>' : '') +
      (l.address ? '<div class="rf-guest-kv rf-guest-kv-full"><span>Address</span><strong>' + esc(l.address) + '</strong></div>' : '') +
      (l.coordinates ? '<div class="rf-guest-kv"><span>Coordinates</span><strong>' + esc(l.coordinates) + '</strong></div>' : '') +
      '<div class="rf-guest-kv rf-guest-kv-full"><span>Description</span><p style="margin:4px 0 0;white-space:pre-wrap;line-height:1.55">' + esc(l.description || 'No description recorded.') + '</p></div>' +
      (l.historical_significance ? '<div class="rf-guest-kv rf-guest-kv-full"><span>Historical Significance</span><p style="margin:4px 0 0;white-space:pre-wrap;line-height:1.55">' + esc(l.historical_significance) + '</p></div>' : '') +
      '</div>' +
      '<h3 style="margin:16px 0 10px;font-size:.92rem">Reference Photos (' + imgs.length + ')</h3>' +
      (imgs.length
        ? '<div class="rf-guest-gallery">' + imgs.map(function (im) {
            return '<figure class="rf-guest-photo" data-rf-zoom="' + esc(apiUrl(im.admin_url)) + '">' +
              '<img src="' + esc(apiUrl(im.admin_url)) + '" alt="' + esc(im.filename || l.name) + '"/>' +
              '<figcaption>' + esc(im.filename || '') + '</figcaption>' +
              '</figure>';
          }).join('') + '</div>'
        : '<div class="rf-empty">No reference photos available.</div>') +
      '</div>';
    document.body.appendChild(wrap);
    wrap.onclick = function (e) { if (e.target === wrap) wrap.remove(); };
    wrap.querySelector('#rf-gl-close').onclick = function () { wrap.remove(); };
    var glEdit = wrap.querySelector('#rf-gl-edit');
    if (glEdit) glEdit.onclick = function () { wrap.remove(); openLocationModal(l); };
    wrap.querySelectorAll('[data-rf-zoom]').forEach(function (el) {
      el.onclick = function () { openViewer(el.getAttribute('data-rf-zoom')); };
    });
  }

  function renderLocations(body) {
    closeMenus();
    var guestMode = isGuest();
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
      if (!q) return true;
      var hay = [l.name, l.group, l.category, l.parent_institution, l.building_or_wing, l.floor_or_room, l.address, l.coordinates, l.description, l.historical_significance, l.notes].join(' ').toLowerCase();
      return hay.indexOf(q) !== -1;
    });

    var folderDropdownHtml =
      '<div class="rf-folder-dd-wrap">' +
      '<label class="rf-dd-label" for="rf-loc-grp-sel">📁 Group Folder</label>' +
      '<select class="rf-select rf-folder-select" id="rf-loc-grp-sel">' +
      '<option value="all"' + (state.selectedLocGroup === 'all' ? ' selected' : '') + '>📂 All Folders (' + state.locations.length + ')</option>' +
      state.locationGroups.map(function (g) {
        var gs = String(g.slug || '').toLowerCase();
        var cnt = state.locations.filter(function (l) { return locGroupSlug(l) === gs; }).length;
        return '<option value="' + esc(gs) + '"' + (state.selectedLocGroup === gs ? ' selected' : '') + '>📁 ' + esc(g.name) + ' (' + cnt + ')</option>';
      }).join('') +
      '<option value="unassigned"' + (state.selectedLocGroup === 'unassigned' ? ' selected' : '') + '>📂 Unassigned (' + unassignedCnt + ')</option>' +
      '</select>' +
      (!guestMode && activeGroupObj ? '<button type="button" class="rf-btn rf-btn-sm" id="rf-loc-grp-manage" title="Rename or delete selected folder">' + DOTS_SVG + '<span>Manage Folder</span></button>' : '') +
      '</div>';

    var groupOpts = function (curSlug) {
      return '<option value=""' + (!curSlug ? ' selected' : '') + '>📂 Unassigned</option>' +
        state.locationGroups.map(function (g) {
          var gs = String(g.slug || '').toLowerCase();
          return '<option value="' + esc(gs) + '"' + (curSlug === gs ? ' selected' : '') + '>📁 ' + esc(g.name) + '</option>';
        }).join('');
    };

    body.innerHTML =
      '<div class="rf-section-head"><div><h2>📍 Archival Locations &amp; Landmarks' + (activeGroupObj ? ' — 📁 ' + esc(activeGroupObj.name) : state.selectedLocGroup === 'unassigned' ? ' — Unassigned' : '') + '</h2>' +
      '<p>' + esc(activeGroupObj && activeGroupObj.description ? activeGroupObj.description : (guestMode ? 'Browse archival halls, wings, rooms, and historical landmarks (read-only).' : 'Organize halls, wings, rooms, and buildings into maintainable group folders. Select a folder from the dropdown to filter or manage it.')) + '</p></div>' +
      (guestMode ? '' : '<div style="display:flex;gap:8px;flex-wrap:wrap">' +
      '<button type="button" class="rf-btn" id="rf-add-lgrp-btn">📁＋ New Group Folder</button>' +
      '<button type="button" class="rf-btn rf-btn-primary" id="rf-add-loc-btn">＋ Add Location</button>' +
      '</div>') + '</div>' +
      '<div class="rf-toolbar rf-toolbar-wrap">' +
      folderDropdownHtml +
      '<input class="rf-input arkib-input arkib-input--underline" id="rf-loc-search" style="flex:1;min-width:220px" type="search" placeholder="Search locations in this folder by name, wing, hall, or address…" value="' + esc(state.search) + '"/>' +
      '</div>' +
      '<div class="rf-grid">' +
      (list.length ? list.map(function (l) {
        var thumb = l.images && l.images[0] ? '<img class="rf-thumb" data-rf-zoom="' + esc(apiUrl(l.images[0].admin_url)) + '" style="cursor:pointer" src="' + esc(apiUrl(l.images[0].admin_url)) + '" alt="' + esc(l.name) + '"/>' : '<div class="rf-thumb">📍</div>';
        var sub = [l.building_or_wing, l.floor_or_room].filter(Boolean).join(' · ') || l.parent_institution || l.category || 'Archival Location';
        var curGs = locGroupSlug(l);
        var revCnt = Number(l.revision_count || 0);
        var actionsHtml = guestMode
          ? '<div class="rf-actions">' +
            '<span class="rf-folder-readonly" title="Group folder (read-only)">' + (l.group ? '📁 ' + esc(l.group) : '📂 Unassigned') + '</span>' +
            '<button type="button" class="rf-btn rf-btn-sm" data-rf-view-l="' + esc(l.slug) + '">' + EYE_SVG + '<span>View Details</span></button>' +
            '</div>'
          : '<div class="rf-actions rf-actions-compact">' +
            '<select class="rf-move-sel" data-rf-move-l="' + esc(l.slug) + '" title="Change group folder">' + groupOpts(curGs) + '</select>' +
            '<button type="button" class="rf-btn rf-card-menu-btn" data-rf-menu-l="' + esc(l.slug) + '" title="Actions (View, Edit, Delete)" aria-label="Location actions">' + DOTS_SVG + '</button>' +
            '</div>';
        return '<article class="rf-card">' + thumb + '<div class="rf-card-body">' +
          '<h3 title="' + esc(l.name) + '">' + esc(l.name) + '</h3>' +
          '<div class="rf-sub" title="' + esc(sub) + '">' + esc(sub) + '</div>' +
          '<p class="rf-desc">' + esc(l.description || l.historical_significance || 'No description yet.') + '</p>' +
          '<div class="rf-meta">' + (l.image_count || 0) + ' photo(s)' + (l.coordinates ? ' · ' + esc(l.coordinates) : '') +
          (!guestMode && revCnt > 0 ? ' · <span class="rf-badge rf-badge-info">🕒 ' + revCnt + ' change' + (revCnt === 1 ? '' : 's') + '</span>' : '') + '</div>' +
          actionsHtml + '</div></article>';
      }).join('') : '<div class="rf-empty">' + (guestMode ? 'No archival locations match your filter.' : 'No archival locations in this folder yet. Move one here using the folder dropdown or click "＋ Add Location".') + '</div>') +
      '</div>';

    var addLgrpBtn = document.getElementById('rf-add-lgrp-btn');
    if (addLgrpBtn) addLgrpBtn.onclick = function () { openGroupModal('location', null); };
    var addLocBtn = document.getElementById('rf-add-loc-btn');
    if (addLocBtn) addLocBtn.onclick = function () { openLocationModal(null); };
    body.querySelectorAll('[data-rf-zoom]').forEach(function (imgEl) {
      imgEl.onclick = function () { openViewer(imgEl.getAttribute('data-rf-zoom')); };
    });
    body.querySelectorAll('[data-rf-view-l]').forEach(function (b) {
      b.onclick = function () {
        var l = state.locations.find(function (x) { return x.slug === b.getAttribute('data-rf-view-l'); });
        if (l) openGuestLocationModal(l);
      };
    });
    document.getElementById('rf-loc-grp-sel').onchange = function (e) {
      state.selectedLocGroup = e.target.value;
      renderLocations(body);
    };
    var manageLocGrp = document.getElementById('rf-loc-grp-manage');
    if (manageLocGrp && activeGroupObj) {
      manageLocGrp.onclick = function (ev) {
        ev.stopPropagation();
        var gs = String(activeGroupObj.slug || '').toLowerCase();
        var memberCnt = state.locations.filter(function (l) { return locGroupSlug(l) === gs; }).length;
        openFolderMenu(manageLocGrp, {
          onRename: function () { openGroupModal('location', activeGroupObj); },
          onDelete: function () {
            openConfirmModal({
              title: 'Delete Group Folder "' + activeGroupObj.name + '"?',
              message: 'This removes the "' + activeGroupObj.name + '" folder. The ' + memberCnt + ' location' + (memberCnt === 1 ? '' : 's') + ' inside will NOT be deleted — they will move safely to Unassigned.',
              confirmLabel: 'Delete Folder',
              onConfirm: async function () {
                var r = await api('/admin/api/location-groups/' + encodeURIComponent(activeGroupObj.slug), { method: 'DELETE' });
                if (r.ok) {
                  state.selectedLocGroup = 'all';
                  toast('Deleted folder "' + activeGroupObj.name + '"');
                  await refreshAll(false);
                } else toast((r.data && r.data.reason) || 'Delete failed', false);
              }
            });
          }
        });
      };
    }
    document.getElementById('rf-loc-search').oninput = function (e) {
      var pos = e.target.selectionStart;
      state.search = e.target.value;
      trackSearchDebounced('location', state.search);
      renderLocations(body);
      restoreFocus('rf-loc-search', pos);
    };
    body.querySelectorAll('[data-rf-rev-l]').forEach(function (b) {
      b.onclick = function () {
        var slug = b.getAttribute('data-rf-rev-l');
        var l = state.locations.find(function (x) { return x.slug === slug; });
        openRevisionsModal('location', slug, l ? l.name : slug);
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
    body.querySelectorAll('[data-rf-menu-l]').forEach(function (btn) {
      btn.onclick = function (ev) {
        ev.stopPropagation();
        var l = state.locations.find(function (x) { return x.slug === btn.getAttribute('data-rf-menu-l'); });
        if (!l) return;
        var revCnt = Number(l.revision_count || 0);
        var items = [
          { label: 'View Details', icon: EYE_SVG, onClick: function () { openGuestLocationModal(l); } },
          { label: 'Edit Location', icon: EDIT_SVG, onClick: function () { openLocationModal(l); } }
        ];
        if (revCnt > 0) {
          items.push({ label: 'View Changes (' + revCnt + ')', icon: '<span>🕒</span>', onClick: function () { openRevisionsModal('location', l.slug, l.name); } });
        }
        items.push({ divider: true });
        items.push({
          label: 'Delete Location',
          icon: TRASH_SVG,
          danger: true,
          onClick: function () {
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
          }
        });
        openCardActionMenu(btn, items);
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
      '<form id="rf-l-form" novalidate><div class="rf-form-grid">' +
      '<div class="rf-field arkib-field"><label class="arkib-label" for="rf-l-name">Location Name</label><input class="rf-input arkib-input" id="rf-l-name" required value="' + esc(loc ? loc.name || '' : '') + '"/></div>' +
      '<div class="rf-field arkib-field"><label class="arkib-label" for="rf-l-group">Group Folder</label><select class="rf-select arkib-input" id="rf-l-group">' + grpSelectHtml + '</select></div>' +
      '<div class="rf-field arkib-field"><label class="arkib-label" for="rf-l-inst">Parent Institution</label><input class="rf-input arkib-input" id="rf-l-inst" value="' + esc(loc ? loc.parent_institution || '' : '') + '"/></div>' +
      '<div class="rf-field arkib-field"><label class="arkib-label" for="rf-l-cat">Category</label><input class="rf-input arkib-input" id="rf-l-cat" value="' + esc(loc ? loc.category || '' : '') + '"/></div>' +
      '<div class="rf-field arkib-field"><label class="arkib-label" for="rf-l-bld">Building / Wing</label><input class="rf-input arkib-input" id="rf-l-bld" value="' + esc(loc ? loc.building_or_wing || '' : '') + '"/></div>' +
      '<div class="rf-field arkib-field"><label class="arkib-label" for="rf-l-flr">Floor / Room</label><input class="rf-input arkib-input" id="rf-l-flr" value="' + esc(loc ? loc.floor_or_room || '' : '') + '"/></div>' +
      '<div class="rf-field arkib-field full"><label class="arkib-label" for="rf-l-addr">Address</label><input class="rf-input arkib-input" id="rf-l-addr" value="' + esc(loc ? loc.address || '' : '') + '"/></div>' +
      '<div class="rf-field arkib-field full"><label class="arkib-label" for="rf-l-gps">GPS Coordinates</label><input class="rf-input arkib-input" id="rf-l-gps" value="' + esc(loc ? loc.coordinates || '' : '') + '"/></div>' +
      '<div class="rf-field arkib-field full"><label class="arkib-label" for="rf-l-hist">Historical Significance</label><textarea class="rf-textarea arkib-input" id="rf-l-hist">' + esc(loc ? loc.historical_significance || '' : '') + '</textarea></div>' +
      '<div class="rf-field arkib-field full"><label class="arkib-label" for="rf-l-desc">Description</label><textarea class="rf-textarea arkib-input" id="rf-l-desc">' + esc(loc ? loc.description || '' : '') + '</textarea><div id="rf-l-desc-ai-slot"></div></div>' +
      '<div class="rf-field arkib-field full"><label class="arkib-label" for="rf-l-notes">Notes</label><textarea class="rf-textarea arkib-input" id="rf-l-notes">' + esc(loc ? loc.notes || '' : '') + '</textarea></div>' +
      (loc ? '<div class="rf-field arkib-field full"><label class="arkib-label">Reference Photos (click photo to view · auto-suggests description if empty)</label><div class="rf-gallery" id="rf-l-gal"></div><input class="rf-input arkib-input" id="rf-l-addimg" type="file" accept=".jpg,.jpeg,.png,.webp,.bmp" style="margin-top:8px"/></div>' : '') +
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
        return '<div class="rf-tile"><img src="' + esc(apiUrl(im.admin_url)) + '" data-fn="' + esc(im.filename) + '" data-zoom="' + esc(apiUrl(im.admin_url)) + '"/><button type="button" class="rf-tile-del" data-del="' + esc(im.filename) + '" title="Delete photo">' + TRASH_SVG + '</button></div>';
      }).join('') : '<div style="font-size:.78rem;color:rgb(var(--text-secondary));padding:10px">No reference photos yet.</div>';
      gal.querySelectorAll('[data-zoom]').forEach(function (img) {
        img.onclick = async function () {
          openViewer(img.getAttribute('data-zoom'));
          var descTa = document.getElementById('rf-l-desc');
          var slot = document.getElementById('rf-l-desc-ai-slot');
          var fn = img.getAttribute('data-fn');
          if (descTa && slot && fn && isMissingDescription(descTa.value)) {
            slot.innerHTML = '<div class="rf-ai-desc-box"><div class="rf-ai-desc-head"><span>✨ AI Description Suggestion</span><span class="rf-badge rf-badge-info">Analyzing…</span></div><div class="rf-ai-desc-text" style="opacity:.8">⏳ Generating description from clicked photo…</div></div>';
            try {
              var res = await api('/admin/api/suggest-image-description', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ entity_type: 'location', slug: cur.slug, filename: fn, context_label: cur.name || cur.slug })
              });
              var sug = res.ok && res.data && res.data.suggested_description ? String(res.data.suggested_description).trim() : '';
              if (!sug) { slot.innerHTML = ''; return; }
              slot.innerHTML =
                '<div class="rf-ai-desc-box">' +
                '<div class="rf-ai-desc-head"><span>✨ AI Suggested Location Description</span><span class="rf-badge rf-badge-ok">' + esc(res.data.source === 'ollama_vision' ? 'Local Vision AI' : 'Metadata Fallback') + '</span></div>' +
                '<div class="rf-ai-desc-text">' + esc(sug) + '</div>' +
                '<div class="rf-ai-desc-actions">' +
                '<button type="button" class="rf-btn rf-btn-primary rf-btn-sm" id="rf-l-ai-apply">✓ Apply to Description</button>' +
                '<button type="button" class="rf-btn rf-btn-sm" id="rf-l-ai-dismiss">✕ Dismiss</button>' +
                '</div></div>';
              document.getElementById('rf-l-ai-apply').onclick = function () { descTa.value = sug; slot.innerHTML = ''; descTa.focus(); toast('AI description applied'); };
              document.getElementById('rf-l-ai-dismiss').onclick = function () { slot.innerHTML = ''; };
            } catch (e) { slot.innerHTML = ''; }
          }
        };
      });
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
      var lForm = ev.target;
      clearFormErrors(lForm);
      var lNameEl = document.getElementById('rf-l-name');
      if (!lNameEl.value.trim()) {
        setFieldError(lNameEl, 'Location name is required.');
        lNameEl.focus();
        return;
      }
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

  function docYearNum(d) {
    var raw = String((d && (d.year || d.gregorian_date_masihi || d.date_created)) || '');
    var m = raw.match(/\b(1\d{3}|20\d{2})\b/);
    return m ? parseInt(m[1], 10) : 0;
  }

  function openGuestDocumentModal(d) {
    if (!d) return;
    var guestMode = isGuest();
    if (guestMode) trackGuestView('document', d.slug, d.title, d.category || '');
    else {
      trackUserPref({
        event_type: 'click_document',
        category: d.category || '',
        source: d.author_or_source || '',
        group_folder: d.group || '',
        year: d.year || '',
        item_slug: d.slug,
        item_title: d.title || d.slug
      });
    }
    var files = Array.isArray(d.files) ? d.files : [];
    var yr = d.year || (docYearNum(d) ? String(docYearNum(d)) : '');
    var expBase = guestMode ? '/guest/api/documents/' : '/admin/api/documents/';
    var wrap = document.createElement('div');
    wrap.className = 'rf-modal-backdrop';
    wrap.innerHTML =
      '<div class="rf-modal" style="width:min(760px,100%)">' +
      '<div class="rf-modal-head"><div><span class="rf-pill ' + (guestMode ? 'rf-pill-guest' : '') + '" style="margin-bottom:6px;display:inline-flex">' + (guestMode ? '👁️ READ-ONLY DOCUMENT' : '👁️ DOCUMENT DETAILS') + '</span><h2>📁 ' + esc(d.title || d.slug) + '</h2></div>' +
      '<div style="display:flex;gap:8px">' +
      (guestMode ? '' : '<button type="button" class="rf-btn rf-btn-primary rf-btn-sm" id="rf-gd-edit">' + EDIT_SVG + '<span>Edit</span></button>') +
      '<button type="button" class="rf-btn rf-btn-sm" id="rf-gd-close">✕ Close</button></div></div>' +
      '<div class="rf-guest-kv-grid">' +
      '<div class="rf-guest-kv"><span>Category</span><strong>' + esc(d.category || 'Uncategorized') + '</strong></div>' +
      '<div class="rf-guest-kv"><span>Group Folder</span><strong>' + esc(d.group || 'Unassigned') + '</strong></div>' +
      (yr ? '<div class="rf-guest-kv"><span>Year / Date</span><strong>' + esc(yr) + '</strong></div>' : '') +
      (d.author_or_source ? '<div class="rf-guest-kv"><span>Author / Source</span><strong>' + esc(d.author_or_source) + '</strong></div>' : '') +
      (d.subject ? '<div class="rf-guest-kv"><span>Subject</span><strong>' + esc(d.subject) + '</strong></div>' : '') +
      (d.script_type ? '<div class="rf-guest-kv"><span>Script / Language</span><strong>' + esc(d.script_type) + '</strong></div>' : '') +
      (d.summary ? '<div class="rf-guest-kv rf-guest-kv-full"><span>Summary</span><p style="margin:4px 0 0;white-space:pre-wrap;line-height:1.55">' + esc(d.summary) + '</p></div>' : '') +
      (d.jawi_text ? '<div class="rf-guest-kv rf-guest-kv-full"><span>Original Jawi Text (Teks Jawi Asal · RTL)</span><pre class="rf-guest-pre rf-jawi-preview" dir="rtl">' + esc(d.jawi_text) + '</pre></div>' : '') +
      (d.extracted_text ? '<div class="rf-guest-kv rf-guest-kv-full"><span>Transcribed / Extracted Text (Rumi)</span><pre class="rf-guest-pre">' + esc(d.extracted_text) + '</pre></div>' : '') +
      '</div>' +
      '<div class="rf-actions" style="margin:14px 0">' +
      '<a class="rf-btn rf-btn-sm" href="' + esc(apiUrl(expBase + encodeURIComponent(d.slug) + '/export/docx')) + '" download="' + esc(d.slug) + '.docx">📄 Download DOCX</a>' +
      '<a class="rf-btn rf-btn-sm" href="' + esc(apiUrl(expBase + encodeURIComponent(d.slug) + '/export/pdf')) + '" download="' + esc(d.slug) + '.pdf">📕 Download PDF</a>' +
      '</div>' +
      '<h3 style="margin:14px 0 10px;font-size:.92rem">Attached Files (' + files.length + ')</h3>' +
      (files.length
        ? '<div class="rf-guest-gallery">' + files.map(function (f) {
            if (f.is_image) {
              return '<figure class="rf-guest-photo" data-rf-zoom="' + esc(apiUrl(f.admin_url)) + '">' +
                '<img src="' + esc(apiUrl(f.admin_url)) + '" alt="' + esc(f.filename) + '"/>' +
                '<figcaption>' + esc(f.filename) + '</figcaption>' +
                '</figure>';
            }
            return '<div class="rf-guest-file-tile">' +
              '<div style="font-size:1.5rem;margin-bottom:6px">📄</div>' +
              '<div style="font-size:.82rem;font-weight:600;word-break:break-word;margin-bottom:8px">' + esc(f.filename) + '</div>' +
              '<a class="rf-btn rf-btn-sm" href="' + esc(apiUrl(f.admin_url)) + '" target="_blank" rel="noopener">↗ Open File</a>' +
              '</div>';
          }).join('') + '</div>'
        : '<div class="rf-empty">No files attached to this document.</div>') +
      '</div>';
    document.body.appendChild(wrap);
    wrap.onclick = function (e) { if (e.target === wrap) wrap.remove(); };
    wrap.querySelector('#rf-gd-close').onclick = function () { wrap.remove(); };
    var gdEdit = wrap.querySelector('#rf-gd-edit');
    if (gdEdit) gdEdit.onclick = function () { wrap.remove(); openDocumentModal(d); };
    wrap.querySelectorAll('[data-rf-zoom]').forEach(function (el) {
      el.onclick = function () { openViewer(el.getAttribute('data-rf-zoom')); };
    });
  }

  function renderDocuments(body) {
    closeMenus();
    var guestMode = isGuest();
    var q = state.search.toLowerCase();
    var validSlugs = {};
    state.documentGroups.forEach(function (g) { validSlugs[String(g.slug || '').toLowerCase()] = true; });
    if (state.selectedDocGroup !== 'all' && state.selectedDocGroup !== 'unassigned' && !validSlugs[state.selectedDocGroup]) {
      state.selectedDocGroup = 'all';
    }

    var unassignedCnt = state.documents.filter(function (d) { return !docGroupSlug(d); }).length;
    var activeGroupObj = state.documentGroups.find(function (g) { return String(g.slug || '').toLowerCase() === state.selectedDocGroup; });

    // Collect distinct sources for source filter dropdown
    var sourceMap = {};
    state.documents.forEach(function (d) {
      var s = String(d.author_or_source || '').trim();
      if (s) sourceMap[s] = (sourceMap[s] || 0) + 1;
    });
    var allSources = Object.keys(sourceMap).sort(function (a, b) { return a.localeCompare(b); });

    var prefScores = (!guestMode && state.preferencesData && state.preferencesData.document_scores) || {};

    var list = state.documents.filter(function (d) {
      var gs = docGroupSlug(d);
      if (state.selectedDocGroup === 'unassigned' && gs) return false;
      if (state.selectedDocGroup !== 'all' && state.selectedDocGroup !== 'unassigned' && gs !== state.selectedDocGroup) return false;
      if (state.docCategoryFilter !== 'all') {
        var catLc = String(d.category || '').trim().toLowerCase();
        if (state.docCategoryFilter === 'uncategorized') {
          if (catLc) return false;
        } else if (catLc !== state.docCategoryFilter.toLowerCase()) {
          return false;
        }
      }
      if (state.docSourceFilter !== 'all') {
        var srcVal = String(d.author_or_source || '').trim();
        if (srcVal !== state.docSourceFilter) return false;
      }
      if (!q) return true;
      var hay = [d.title, d.subject, d.category, d.year, d.author_or_source, d.group, d.script_type, d.reference_code, d.origin_location, d.summary, d.jawi_text, String(d.extracted_text || '').slice(0, 600), d.notes].join(' ').toLowerCase();
      return hay.indexOf(q) !== -1;
    });

    list.sort(function (a, b) {
      if (state.docSort === 'category') {
        return String(a.category || 'zzz').localeCompare(String(b.category || 'zzz')) || String(a.title || '').localeCompare(String(b.title || ''));
      }
      if (state.docSort === 'year_desc') {
        return (docYearNum(b) - docYearNum(a)) || String(a.title || '').localeCompare(String(b.title || ''));
      }
      if (state.docSort === 'year_asc') {
        var ya = docYearNum(a) || 99999;
        var yb = docYearNum(b) || 99999;
        return (ya - yb) || String(a.title || '').localeCompare(String(b.title || ''));
      }
      if (state.docSort === 'source') {
        return String(a.author_or_source || 'zzz').localeCompare(String(b.author_or_source || 'zzz')) || String(a.title || '').localeCompare(String(b.title || ''));
      }
      if (state.docSort === 'title') {
        return String(a.title || '').localeCompare(String(b.title || ''));
      }
      if (!guestMode && state.docSort === 'for_you') {
        var sa = Number(prefScores[a.slug] || 0);
        var sb = Number(prefScores[b.slug] || 0);
        return (sb - sa) || String(b.updated_at || '').localeCompare(String(a.updated_at || ''));
      }
      return String(b.updated_at || b.created_at || '').localeCompare(String(a.updated_at || a.created_at || ''));
    });

    var folderDropdownHtml =
      '<div class="rf-folder-dd-wrap">' +
      '<label class="rf-dd-label" for="rf-doc-grp-sel">📁 Folder</label>' +
      '<select class="rf-select rf-folder-select" id="rf-doc-grp-sel">' +
      '<option value="all"' + (state.selectedDocGroup === 'all' ? ' selected' : '') + '>📂 All Folders (' + state.documents.length + ')</option>' +
      state.documentGroups.map(function (g) {
        var gs = String(g.slug || '').toLowerCase();
        var cnt = state.documents.filter(function (d) { return docGroupSlug(d) === gs; }).length;
        return '<option value="' + esc(gs) + '"' + (state.selectedDocGroup === gs ? ' selected' : '') + '>📁 ' + esc(g.name) + ' (' + cnt + ')</option>';
      }).join('') +
      '<option value="unassigned"' + (state.selectedDocGroup === 'unassigned' ? ' selected' : '') + '>📂 Unassigned (' + unassignedCnt + ')</option>' +
      '</select>' +
      (!guestMode && activeGroupObj ? '<button type="button" class="rf-btn rf-btn-sm" id="rf-doc-grp-manage" title="Rename or delete selected folder">' + DOTS_SVG + '</button>' : '') +
      '</div>';

    var catFilterHtml =
      '<div class="rf-folder-dd-wrap">' +
      '<label class="rf-dd-label" for="rf-doc-cat-filter">🏷️ Category</label>' +
      '<select class="rf-select rf-compact-select" id="rf-doc-cat-filter">' +
      '<option value="all"' + (state.docCategoryFilter === 'all' ? ' selected' : '') + '>All Categories</option>' +
      state.categories.map(function (c) {
        return '<option value="' + esc(c.name) + '"' + (state.docCategoryFilter === c.name ? ' selected' : '') + '>' + esc(c.name) + ' (' + Number(c.document_count || 0) + ')</option>';
      }).join('') +
      '<option value="uncategorized"' + (state.docCategoryFilter === 'uncategorized' ? ' selected' : '') + '>Uncategorized</option>' +
      '</select></div>';

    var srcFilterHtml =
      '<div class="rf-folder-dd-wrap">' +
      '<label class="rf-dd-label" for="rf-doc-src-filter">🏛️ Source</label>' +
      '<select class="rf-select rf-compact-select" id="rf-doc-src-filter">' +
      '<option value="all"' + (state.docSourceFilter === 'all' ? ' selected' : '') + '>All Sources</option>' +
      allSources.map(function (s) {
        return '<option value="' + esc(s) + '"' + (state.docSourceFilter === s ? ' selected' : '') + '>' + esc(s) + ' (' + sourceMap[s] + ')</option>';
      }).join('') +
      '</select></div>';

    var sortHtml =
      '<div class="rf-folder-dd-wrap">' +
      '<label class="rf-dd-label" for="rf-doc-sort-sel">↕️ Sort</label>' +
      '<select class="rf-select rf-compact-select" id="rf-doc-sort-sel">' +
      '<option value="newest"' + (state.docSort === 'newest' ? ' selected' : '') + '>Newest Updated</option>' +
      (guestMode ? '' : '<option value="for_you"' + (state.docSort === 'for_you' ? ' selected' : '') + '>⭐ For You (Preferences)</option>') +
      '<option value="category"' + (state.docSort === 'category' ? ' selected' : '') + '>Category (A–Z)</option>' +
      '<option value="year_desc"' + (state.docSort === 'year_desc' ? ' selected' : '') + '>Year (Newest First)</option>' +
      '<option value="year_asc"' + (state.docSort === 'year_asc' ? ' selected' : '') + '>Year (Oldest First)</option>' +
      '<option value="source"' + (state.docSort === 'source' ? ' selected' : '') + '>Source (A–Z)</option>' +
      '<option value="title"' + (state.docSort === 'title' ? ' selected' : '') + '>Title (A–Z)</option>' +
      '</select></div>';

    var groupOpts = function (curSlug) {
      return '<option value=""' + (!curSlug ? ' selected' : '') + '>📂 Unassigned</option>' +
        state.documentGroups.map(function (g) {
          var gs = String(g.slug || '').toLowerCase();
          return '<option value="' + esc(gs) + '"' + (curSlug === gs ? ' selected' : '') + '>📁 ' + esc(g.name) + '</option>';
        }).join('');
    };

    // Personalized recommendation bar based on click preferences (Admin only)
    var recs = (!guestMode && state.preferencesData && state.preferencesData.recommended_documents) || [];
    var prefRecHtml = recs.length ? (
      '<div class="rf-pref-strip">' +
      '<div class="rf-pref-strip-head"><span>⭐ Recommended Based on Your Activity</span>' +
      '<button type="button" class="rf-btn rf-btn-sm" id="rf-open-prefs-inline">⚙️ Customize Preferences</button></div>' +
      '<div class="rf-pref-cards">' +
      recs.slice(0, 4).map(function (r) {
        return '<button type="button" class="rf-pref-chip" data-rf-pref-open="' + esc(r.slug) + '">' +
          '<div class="rf-pref-chip-title">' + esc(r.title || r.slug) + '</div>' +
          '<div class="rf-pref-chip-meta">' + esc((r.match_reasons || []).slice(0, 2).join(' · ') || 'Related item') + '</div>' +
          '</button>';
      }).join('') +
      '</div></div>'
    ) : '';

    var exportBase = guestMode ? '/guest/api/documents/' : '/admin/api/documents/';

    body.innerHTML =
      '<div class="rf-section-head"><div><h2>📁 Archival Documents &amp; Manuscripts' + (activeGroupObj ? ' — 📁 ' + esc(activeGroupObj.name) : state.selectedDocGroup === 'unassigned' ? ' — Unassigned' : '') + '</h2>' +
      '<p>' + esc(activeGroupObj && activeGroupObj.description ? activeGroupObj.description : (guestMode ? 'Browse, filter, and download digitized archival records by category, year, or source (read-only).' : 'Organize, filter, and sort digitized records by category, year, source, or your learned preferences.')) + '</p></div>' +
      (guestMode ? '' : '<div style="display:flex;gap:8px;flex-wrap:wrap">' +
      '<button type="button" class="rf-btn" id="rf-manage-cats-btn">🏷️ Categories</button>' +
      '<button type="button" class="rf-btn" id="rf-my-prefs-btn">⭐ My Preferences</button>' +
      '<button type="button" class="rf-btn" id="rf-add-dgrp-btn">📁＋ New Folder</button>' +
      '<button type="button" class="rf-btn rf-btn-primary" id="rf-add-doc-btn">＋ Add Document</button>' +
      '</div>') + '</div>' +
      prefRecHtml +
      '<div class="rf-toolbar rf-toolbar-wrap">' +
      folderDropdownHtml + catFilterHtml + srcFilterHtml + sortHtml +
      '<input class="rf-input arkib-input arkib-input--underline" id="rf-doc-search" style="flex:1;min-width:200px" type="search" placeholder="Search title, category, year, source, or text…" value="' + esc(state.search) + '"/>' +
      '</div>' +
      '<div class="rf-grid">' +
      (list.length ? list.map(function (d) {
        var yr = d.year || (docYearNum(d) ? String(docYearNum(d)) : '');
        var sub = [d.category, yr, d.author_or_source, d.script_type].filter(Boolean).join(' · ') || 'Archival Document';
        var verdict = d.authenticity_report && d.authenticity_report.verdict ? '<span class="rf-badge rf-badge-warn" style="margin-left:6px">' + esc(d.authenticity_report.verdict) + '</span>' : '';
        var revCnt = Number(d.revision_count || 0);
        var revBadge = (!guestMode && revCnt > 0) ? '<span class="rf-badge rf-badge-info" style="margin-left:6px">🕒 ' + revCnt + ' change' + (revCnt === 1 ? '' : 's') + '</span>' : '';
        var curGs = docGroupSlug(d);
        var firstImg = (d.files || []).find(function (f) { return f.is_image; });
        var thumbHtml = firstImg
          ? '<img class="rf-thumb" data-rf-zoom="' + esc(apiUrl(firstImg.admin_url)) + '" style="cursor:pointer" src="' + esc(apiUrl(firstImg.admin_url)) + '" alt="' + esc(d.title) + '"/>'
          : '<div class="rf-thumb">📁</div>';
        var actionsHtml = guestMode
          ? '<div class="rf-actions rf-actions-compact">' +
            '<span class="rf-folder-readonly" title="Group folder (read-only)">' + (d.group ? '📁 ' + esc(d.group) : '📂 Unassigned') + '</span>' +
            '<button type="button" class="rf-btn rf-btn-sm" data-rf-view-d="' + esc(d.slug) + '">' + EYE_SVG + '<span>View Details</span></button>' +
            '<button type="button" class="rf-btn rf-card-menu-btn" data-rf-menu-dg="' + esc(d.slug) + '" title="Download options (DOCX, PDF)" aria-label="Download options">' + DOTS_SVG + '</button>' +
            '</div>'
          : '<div class="rf-actions rf-actions-compact">' +
            '<select class="rf-move-sel" data-rf-move-d="' + esc(d.slug) + '" title="Change group folder">' + groupOpts(curGs) + '</select>' +
            '<button type="button" class="rf-btn rf-card-menu-btn" data-rf-menu-d="' + esc(d.slug) + '" title="Actions (View, Edit, Export, Delete)" aria-label="Document actions">' + DOTS_SVG + '</button>' +
            '</div>';
        return '<article class="rf-card">' + thumbHtml + '<div class="rf-card-body">' +
          '<h3 title="' + esc(d.title) + '">' + esc(d.title) + '</h3>' +
          '<div class="rf-sub" title="' + esc(sub) + '">' + esc(sub) + '</div>' +
          (d.jawi_text ? '<p class="rf-desc rf-jawi-card-snippet" dir="rtl" title="Preserved Original Jawi Text">' + esc(d.jawi_text) + '</p>' : '') +
          '<p class="rf-desc">' + esc(d.summary || d.extracted_text || 'No summary yet.') + '</p>' +
          '<div class="rf-meta">' + (d.file_count || 0) + ' file(s)' + verdict + revBadge + '</div>' +
          actionsHtml + '</div></article>';
      }).join('') : '<div class="rf-empty">' + (guestMode ? 'No archival documents match your filters.' : 'No archival documents match your filters. Click "＋ Add Document" to create one.') + '</div>') +
      '</div>';

    var manageCatsBtn = document.getElementById('rf-manage-cats-btn');
    if (manageCatsBtn) manageCatsBtn.onclick = function () { openCategoriesModal(); };
    var myPrefsBtn = document.getElementById('rf-my-prefs-btn');
    if (myPrefsBtn) myPrefsBtn.onclick = function () { openPreferencesModal(); };
    var inlinePrefs = document.getElementById('rf-open-prefs-inline');
    if (inlinePrefs) inlinePrefs.onclick = function () { openPreferencesModal(); };
    var addDgrpBtn = document.getElementById('rf-add-dgrp-btn');
    if (addDgrpBtn) addDgrpBtn.onclick = function () { openGroupModal('document', null); };
    var addDocBtn = document.getElementById('rf-add-doc-btn');
    if (addDocBtn) addDocBtn.onclick = function () { openDocumentModal(null); };
    body.querySelectorAll('[data-rf-zoom]').forEach(function (imgEl) {
      imgEl.onclick = function () { openViewer(imgEl.getAttribute('data-rf-zoom')); };
    });
    body.querySelectorAll('[data-rf-view-d]').forEach(function (b) {
      b.onclick = function () {
        var d = state.documents.find(function (x) { return x.slug === b.getAttribute('data-rf-view-d'); });
        if (d) openGuestDocumentModal(d);
      };
    });

    document.getElementById('rf-doc-grp-sel').onchange = function (e) {
      state.selectedDocGroup = e.target.value;
      if (state.selectedDocGroup !== 'all' && state.selectedDocGroup !== 'unassigned') {
        var grpObj = state.documentGroups.find(function (x) { return String(x.slug || '').toLowerCase() === state.selectedDocGroup; });
        trackUserPref({ event_type: 'filter_folder', group_folder: grpObj ? grpObj.name : state.selectedDocGroup });
      }
      renderDocuments(body);
    };
    var manageDocGrp = document.getElementById('rf-doc-grp-manage');
    if (manageDocGrp && activeGroupObj) {
      manageDocGrp.onclick = function (ev) {
        ev.stopPropagation();
        var gs = String(activeGroupObj.slug || '').toLowerCase();
        var memberCnt = state.documents.filter(function (d) { return docGroupSlug(d) === gs; }).length;
        openFolderMenu(manageDocGrp, {
          onRename: function () { openGroupModal('document', activeGroupObj); },
          onDelete: function () {
            openConfirmModal({
              title: 'Delete Group Folder "' + activeGroupObj.name + '"?',
              message: 'This removes the "' + activeGroupObj.name + '" folder. The ' + memberCnt + ' document' + (memberCnt === 1 ? '' : 's') + ' inside will NOT be deleted — they will move safely to Unassigned.',
              confirmLabel: 'Delete Folder',
              onConfirm: async function () {
                var r = await api('/admin/api/document-groups/' + encodeURIComponent(activeGroupObj.slug), { method: 'DELETE' });
                if (r.ok) {
                  state.selectedDocGroup = 'all';
                  toast('Deleted folder "' + activeGroupObj.name + '"');
                  await refreshAll(false);
                } else toast((r.data && r.data.reason) || 'Delete failed', false);
              }
            });
          }
        });
      };
    }

    document.getElementById('rf-doc-cat-filter').onchange = function (e) {
      state.docCategoryFilter = e.target.value;
      if (state.docCategoryFilter !== 'all' && state.docCategoryFilter !== 'uncategorized') {
        trackUserPref({ event_type: 'filter_category', category: state.docCategoryFilter });
      }
      renderDocuments(body);
    };
    document.getElementById('rf-doc-src-filter').onchange = function (e) {
      state.docSourceFilter = e.target.value;
      if (state.docSourceFilter !== 'all') {
        trackUserPref({ event_type: 'filter_source', source: state.docSourceFilter });
      }
      renderDocuments(body);
    };
    document.getElementById('rf-doc-sort-sel').onchange = function (e) {
      state.docSort = e.target.value;
      renderDocuments(body);
    };
    document.getElementById('rf-doc-search').oninput = function (e) {
      var pos = e.target.selectionStart;
      state.search = e.target.value;
      trackSearchDebounced('document', state.search);
      renderDocuments(body);
      restoreFocus('rf-doc-search', pos);
    };

    body.querySelectorAll('[data-rf-pref-open]').forEach(function (b) {
      b.onclick = function () {
        var slug = b.getAttribute('data-rf-pref-open');
        var d = state.documents.find(function (x) { return x.slug === slug; });
        if (d) {
          trackUserPref({
            event_type: 'click_document',
            category: d.category || '',
            source: d.author_or_source || '',
            group_folder: d.group || '',
            year: d.year || '',
            tags: Array.isArray(d.key_topics) ? d.key_topics : [],
            item_slug: d.slug,
            item_title: d.title || ''
          });
          openDocumentModal(d);
        }
      };
    });
    body.querySelectorAll('[data-rf-rev-d]').forEach(function (b) {
      b.onclick = function () {
        var slug = b.getAttribute('data-rf-rev-d');
        var d = state.documents.find(function (x) { return x.slug === slug; });
        openRevisionsModal('document', slug, d ? d.title : slug);
      };
    });
    body.querySelectorAll('[data-rf-exp-d]').forEach(function (a) {
      a.addEventListener('click', function () {
        var slug = a.getAttribute('data-rf-exp-d');
        var d = state.documents.find(function (x) { return x.slug === slug; });
        if (d) {
          trackUserPref({
            event_type: 'export',
            category: d.category || '',
            source: d.author_or_source || '',
            group_folder: d.group || '',
            year: d.year || '',
            item_slug: d.slug,
            item_title: d.title || ''
          });
        }
      });
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
    body.querySelectorAll('[data-rf-menu-dg]').forEach(function (btn) {
      btn.onclick = function (ev) {
        ev.stopPropagation();
        var d = state.documents.find(function (x) { return x.slug === btn.getAttribute('data-rf-menu-dg'); });
        if (!d) return;
        openCardActionMenu(btn, [
          { label: 'View Details', icon: EYE_SVG, onClick: function () { openGuestDocumentModal(d); } },
          { label: 'Download DOCX', icon: '<span>📄</span>', href: apiUrl(exportBase + encodeURIComponent(d.slug) + '/export/docx'), download: d.slug + '.docx' },
          { label: 'Download PDF', icon: '<span>📕</span>', href: apiUrl(exportBase + encodeURIComponent(d.slug) + '/export/pdf'), download: d.slug + '.pdf' }
        ]);
      };
    });
    body.querySelectorAll('[data-rf-menu-d]').forEach(function (btn) {
      btn.onclick = function (ev) {
        ev.stopPropagation();
        var d = state.documents.find(function (x) { return x.slug === btn.getAttribute('data-rf-menu-d'); });
        if (!d) return;
        var revCnt = Number(d.revision_count || 0);
        var trackExp = function () {
          trackUserPref({
            event_type: 'export',
            category: d.category || '',
            source: d.author_or_source || '',
            group_folder: d.group || '',
            year: d.year || '',
            item_slug: d.slug,
            item_title: d.title || ''
          });
        };
        var items = [
          { label: 'View Details', icon: EYE_SVG, onClick: function () { openGuestDocumentModal(d); } },
          {
            label: 'Edit Document',
            icon: EDIT_SVG,
            onClick: function () {
              trackUserPref({
                event_type: 'click_document',
                category: d.category || '',
                source: d.author_or_source || '',
                group_folder: d.group || '',
                year: d.year || '',
                tags: Array.isArray(d.key_topics) ? d.key_topics : [],
                item_slug: d.slug,
                item_title: d.title || ''
              });
              openDocumentModal(d);
            }
          },
          { label: 'Download DOCX', icon: '<span>📄</span>', href: apiUrl(exportBase + encodeURIComponent(d.slug) + '/export/docx'), download: d.slug + '.docx', onClick: trackExp },
          { label: 'Download PDF', icon: '<span>📕</span>', href: apiUrl(exportBase + encodeURIComponent(d.slug) + '/export/pdf'), download: d.slug + '.pdf', onClick: trackExp }
        ];
        if (revCnt > 0) {
          items.push({ label: 'View Changes (' + revCnt + ')', icon: '<span>🕒</span>', onClick: function () { openRevisionsModal('document', d.slug, d.title || d.slug); } });
        }
        items.push({ divider: true });
        items.push({
          label: 'Delete Document',
          icon: TRASH_SVG,
          danger: true,
          onClick: function () {
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
          }
        });
        openCardActionMenu(btn, items);
      };
    });
  }

  function openDocumentModal(doc) {
    var wrap = document.createElement('div');
    wrap.className = 'rf-modal-backdrop';
    var scripts = ['Jawi', 'Pallava', 'Kawi', 'Rencong', 'Rumi / Bahasa Melayu', 'English', 'Multilingual'];
    var pendingInitialFile = null;
    var lastSuggestions = null;

    var prefSummary = (state.preferencesData && state.preferencesData.preferences) || {};
    var topCat = (prefSummary.top_categories && prefSummary.top_categories[0] && prefSummary.top_categories[0].value) || '';
    var topSrc = (prefSummary.top_sources && prefSummary.top_sources[0] && prefSummary.top_sources[0].value) || '';
    var topFld = (prefSummary.top_folders && prefSummary.top_folders[0] && prefSummary.top_folders[0].value) || '';

    var defaultGrp = doc
      ? docGroupSlug(doc)
      : (state.selectedDocGroup !== 'all' && state.selectedDocGroup !== 'unassigned'
        ? state.selectedDocGroup
        : (topFld ? (state.documentGroups.find(function (g) { return g.name === topFld; }) || {}).slug || '' : ''));
    var grpSelectHtml =
      '<option value=""' + (!defaultGrp ? ' selected' : '') + '>Unassigned</option>' +
      state.documentGroups.map(function (g) {
        var gs = String(g.slug || '').toLowerCase();
        return '<option value="' + esc(gs) + '"' + (defaultGrp === gs ? ' selected' : '') + '>' + esc(g.name) + '</option>';
      }).join('');

    var curCat = doc ? (doc.category || '') : topCat;
    var catOptionsHtml = '<option value="">— Select Category —</option>' +
      state.categories.map(function (c) {
        var sel = curCat && c.name.toLowerCase() === curCat.toLowerCase();
        return '<option value="' + esc(c.name) + '"' + (sel ? ' selected' : '') + '>' + esc(c.name) + '</option>';
      }).join('');
    if (curCat && !state.categories.some(function (c) { return c.name.toLowerCase() === curCat.toLowerCase(); })) {
      catOptionsHtml += '<option value="' + esc(curCat) + '" selected>' + esc(curCat) + '</option>';
    }

    var knownSources = {};
    if (topSrc) knownSources[topSrc] = true;
    (prefSummary.top_sources || []).forEach(function (x) { if (x.value) knownSources[x.value] = true; });
    state.documents.forEach(function (d) { if (d.author_or_source) knownSources[d.author_or_source] = true; });
    var sourceDatalistHtml = Object.keys(knownSources).map(function (s) {
      return '<option value="' + esc(s) + '"></option>';
    }).join('');

    wrap.innerHTML =
      '<div class="rf-modal"><h2>' + (doc ? 'Edit Document' : 'Add Archival Document') + '</h2>' +
      '<div class="rf-ai-suggest-box">' +
      '<div class="rf-ai-suggest-head"><strong>✨ AI Content Auto-Fill</strong><span>Upload an image, PDF, DOCX, or text file — AI will read it and suggest Category, Subject, Year, and Source.</span></div>' +
      '<div style="display:flex;gap:8px;align-items:center;flex-wrap:wrap">' +
      '<input class="rf-input" id="rf-d-aifile" type="file" style="flex:1;min-width:200px" accept=".txt,.md,.csv,.json,.html,.htm,.xml,.pdf,.docx,.jpg,.jpeg,.png,.webp,.bmp"/>' +
      '<button type="button" class="rf-btn rf-btn-primary rf-btn-sm" id="rf-d-analyze-btn">✨ Analyze &amp; Suggest Fields</button>' +
      '</div>' +
      '<div id="rf-d-ai-results" style="margin-top:8px"></div>' +
      '</div>' +
      '<form id="rf-d-form" novalidate><div class="rf-form-grid">' +
      '<div class="rf-field arkib-field"><label class="arkib-label" for="rf-d-title">Subject / Title</label><input class="rf-input arkib-input" id="rf-d-title" required value="' + esc(doc ? doc.title || '' : '') + '"/></div>' +
      '<div class="rf-field"><label>Group Folder</label><select class="rf-select" id="rf-d-group">' + grpSelectHtml + '</select></div>' +
      '<div class="rf-field"><label>Category</label><select class="rf-select" id="rf-d-cat">' + catOptionsHtml + '</select></div>' +
      '<div class="rf-field"><label>Year (Masihi)</label><input class="rf-input" id="rf-d-year" placeholder="e.g. 1957" value="' + esc(doc ? (doc.year || (docYearNum(doc) ? String(docYearNum(doc)) : '')) : '') + '"/></div>' +
      '<div class="rf-field"><label>Source / Agency / Donor</label><input class="rf-input" id="rf-d-auth" list="rf-d-src-list" placeholder="e.g. Arkib Negara Malaysia" value="' + esc(doc ? doc.author_or_source || '' : topSrc) + '"/><datalist id="rf-d-src-list">' + sourceDatalistHtml + '</datalist></div>' +
      '<div class="rf-field"><label>Reference Code</label><input class="rf-input" id="rf-d-ref" value="' + esc(doc ? doc.reference_code || '' : '') + '"/></div>' +
      '<div class="rf-field"><label>Script / Writing System</label><select class="rf-select" id="rf-d-script">' +
      scripts.map(function (s) { return '<option value="' + esc(s) + '"' + (doc && doc.script_type && doc.script_type.toLowerCase() === s.toLowerCase() ? ' selected' : '') + '>' + esc(s) + '</option>'; }).join('') +
      '</select></div>' +
      '<div class="rf-field"><label>Full Date / Era</label><input class="rf-input" id="rf-d-date" placeholder="e.g. 31 Aug 1957 or 13 Muharam 1324H" value="' + esc(doc ? (doc.date_created || [doc.hijri_date, doc.gregorian_date_masihi].filter(Boolean).join(' · ')) : '') + '"/></div>' +
      '<div class="rf-field full"><label>Origin / Location</label><input class="rf-input" id="rf-d-orig" value="' + esc(doc ? doc.origin_location || '' : '') + '"/></div>' +
      (doc ? '<div class="rf-field full"><label>Uploaded Files (duplicate &amp; change-tracked)</label><div id="rf-d-files" style="display:flex;flex-wrap:wrap;gap:8px;margin-bottom:6px"></div><input class="rf-input" id="rf-d-addfile" type="file" accept=".txt,.md,.csv,.json,.html,.htm,.xml,.pdf,.docx,.jpg,.jpeg,.png,.webp,.bmp"/><div id="rf-d-authinfo" style="font-size:.8rem;margin-top:6px"></div></div>' : '') +
      '<div class="rf-field full"><div style="display:flex;justify-content:space-between;align-items:center"><label>Summary</label>' + (doc ? '<button type="button" class="rf-btn rf-btn-ok rf-btn-sm" id="rf-d-sum-btn">✨ Auto-Summarize</button>' : '') + '</div><textarea class="rf-textarea" id="rf-d-sum">' + esc(doc ? doc.summary || '' : '') + '</textarea></div>' +
      '<div class="rf-field arkib-field full"><div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:6px"><label class="arkib-label" for="rf-d-jawi" style="margin-bottom:0">✍️ Original Jawi / Script Text (Teks Jawi Asal · RTL — Preserved)</label><button type="button" class="rf-btn rf-btn-sm" id="rf-d-decode-jawi-btn" title="Transliterate Jawi into Rumi below">🔄 Decode Jawi → Rumi</button></div><textarea class="rf-textarea arkib-input rf-jawi-doc-ta" id="rf-d-jawi" dir="rtl" placeholder="توليس اتاو سيمڤن تيکس جاوي اصل دسيني...">' + esc(doc ? doc.jawi_text || '' : '') + '</textarea></div>' +
      '<div class="rf-field arkib-field full"><label class="arkib-label" for="rf-d-text">Digitized / Transcribed Text (Rumi)</label><textarea class="rf-textarea arkib-input" id="rf-d-text" style="min-height:110px">' + esc(doc ? doc.extracted_text || '' : '') + '</textarea></div>' +
      '<div class="rf-field"><label>Key Topics (comma-separated)</label><input class="rf-input" id="rf-d-topics" value="' + esc(doc && Array.isArray(doc.key_topics) ? doc.key_topics.join(', ') : '') + '"/></div>' +
      '<div class="rf-field"><label>Key Names (comma-separated)</label><input class="rf-input" id="rf-d-names" value="' + esc(doc && Array.isArray(doc.key_names) ? doc.key_names.join(', ') : '') + '"/></div>' +
      '<div class="rf-field full"><label>Key Dates (comma-separated)</label><input class="rf-input" id="rf-d-dates" value="' + esc(doc && Array.isArray(doc.key_dates) ? doc.key_dates.join(', ') : '') + '"/></div>' +
      '<div class="rf-field full"><label>Notes</label><textarea class="rf-textarea" id="rf-d-notes">' + esc(doc ? doc.notes || '' : '') + '</textarea></div>' +
      '</div><div class="rf-actions" style="justify-content:space-between;margin-top:16px">' +
      (doc && Number(doc.revision_count || 0) > 0 ? '<button type="button" class="rf-btn" id="rf-d-compare-btn">🕒 Compare Changes (' + doc.revision_count + ')</button>' : '<span></span>') +
      '<div style="display:flex;gap:8px"><button type="button" class="rf-btn" id="rf-d-cancel">Cancel</button>' +
      '<button type="submit" class="rf-btn rf-btn-primary">Save Document</button></div></div></form></div>';

    document.body.appendChild(wrap);
    wrap.onclick = function (e) { if (e.target === wrap) wrap.remove(); };
    document.getElementById('rf-d-cancel').onclick = function () { wrap.remove(); };
    var cmpBtn = document.getElementById('rf-d-compare-btn');
    if (cmpBtn && doc) {
      cmpBtn.onclick = function () { openRevisionsModal('document', doc.slug, doc.title || doc.slug); };
    }

    function applySuggestedField(field, val) {
      if (!val) return;
      if (field === 'category') {
        var sel = document.getElementById('rf-d-cat');
        var found = false;
        for (var i = 0; i < sel.options.length; i++) {
          if (sel.options[i].value.toLowerCase() === String(val).toLowerCase()) {
            sel.selectedIndex = i;
            found = true;
            break;
          }
        }
        if (!found) {
          var opt = document.createElement('option');
          opt.value = val;
          opt.textContent = val;
          opt.selected = true;
          sel.appendChild(opt);
        }
      } else if (field === 'title' || field === 'subject') {
        document.getElementById('rf-d-title').value = val;
      } else if (field === 'year') {
        document.getElementById('rf-d-year').value = val;
        if (!document.getElementById('rf-d-date').value.trim()) {
          document.getElementById('rf-d-date').value = val;
        }
      } else if (field === 'author_or_source' || field === 'source') {
        document.getElementById('rf-d-auth').value = val;
      } else if (field === 'summary') {
        document.getElementById('rf-d-sum').value = val;
      } else if (field === 'script_type') {
        var scSel = document.getElementById('rf-d-script');
        for (var j = 0; j < scSel.options.length; j++) {
          if (scSel.options[j].value.toLowerCase().indexOf(String(val).toLowerCase()) !== -1) {
            scSel.selectedIndex = j;
            break;
          }
        }
      }
    }

    var aiFileInput = document.getElementById('rf-d-aifile');
    var aiBtn = document.getElementById('rf-d-analyze-btn');
    var aiOut = document.getElementById('rf-d-ai-results');
    async function runAiUploadAnalysis() {
      var f = aiFileInput.files && aiFileInput.files[0];
      if (!f) { toast('Choose an image or document file first', false); return; }
      pendingInitialFile = f;
      aiBtn.disabled = true;
      aiBtn.textContent = '⏳ Analyzing…';
      aiOut.innerHTML = '<div style="font-size:.82rem;color:rgb(var(--text-secondary))">Reading file content and generating field suggestions…</div>';
      try {
        var fd = new FormData();
        fd.append('file', f);
        var r = await api('/admin/api/analyze-upload', { method: 'POST', body: fd });
        if (!r.ok) {
          aiOut.innerHTML = '<div class="rf-err-banner">' + esc((r.data && r.data.reason) || 'Analysis failed') + '</div>';
          return;
        }
        lastSuggestions = r.data;
        var fields = Array.isArray(r.data.suggested_fields) ? r.data.suggested_fields : [];
        aiOut.innerHTML =
          '<div class="rf-ai-chips-head">' +
          '<span>Suggested fields from <b>' + esc(f.name) + '</b> (' + esc(r.data.analysis_mode === 'ai_and_deterministic' ? 'AI + Rules' : 'Fast Rules') + ')</span>' +
          '<button type="button" class="rf-btn rf-btn-ok rf-btn-sm" id="rf-d-apply-all">✓ Apply All Suggestions</button>' +
          '</div>' +
          '<div class="rf-ai-chips">' +
          fields.map(function (sf, idx) {
            return '<div class="rf-ai-chip">' +
              '<div><div class="rf-ai-chip-lbl">' + esc(sf.label) + ' <span class="rf-badge rf-badge-info">' + esc(sf.confidence) + '</span></div>' +
              '<div class="rf-ai-chip-val">' + esc(sf.value) + '</div>' +
              '<div class="rf-ai-chip-why">' + esc(sf.reason) + '</div></div>' +
              '<button type="button" class="rf-btn rf-btn-sm" data-rf-apply-one="' + idx + '">Apply</button>' +
              '</div>';
          }).join('') +
          '</div>';
        document.getElementById('rf-d-apply-all').onclick = function () {
          fields.forEach(function (sf) { applySuggestedField(sf.field, sf.value); });
          if (lastSuggestions.extracted_text && !document.getElementById('rf-d-text').value.trim()) {
            document.getElementById('rf-d-text').value = lastSuggestions.extracted_text;
          }
          if (Array.isArray(lastSuggestions.key_topics) && lastSuggestions.key_topics.length && !document.getElementById('rf-d-topics').value.trim()) {
            document.getElementById('rf-d-topics').value = lastSuggestions.key_topics.join(', ');
          }
          if (Array.isArray(lastSuggestions.key_names) && lastSuggestions.key_names.length && !document.getElementById('rf-d-names').value.trim()) {
            document.getElementById('rf-d-names').value = lastSuggestions.key_names.join(', ');
          }
          if (Array.isArray(lastSuggestions.key_dates) && lastSuggestions.key_dates.length && !document.getElementById('rf-d-dates').value.trim()) {
            document.getElementById('rf-d-dates').value = lastSuggestions.key_dates.join(', ');
          }
          toast('Applied all AI suggestions');
        };
        aiOut.querySelectorAll('[data-rf-apply-one]').forEach(function (b) {
          b.onclick = function () {
            var sf = fields[Number(b.getAttribute('data-rf-apply-one'))];
            if (sf) { applySuggestedField(sf.field, sf.value); toast('Applied ' + sf.label); }
          };
        });
      } catch (e) {
        aiOut.innerHTML = '<div class="rf-err-banner">Could not analyze file.</div>';
      } finally {
        aiBtn.disabled = false;
        aiBtn.textContent = '✨ Analyze & Suggest Fields';
      }
    }
    aiBtn.onclick = runAiUploadAnalysis;

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
            message: 'Delete file "' + fn + '" from document "' + cur.title + '"? A snapshot will remain in Change History.',
            confirmLabel: 'Delete File',
            onConfirm: async function () {
              var r = await api('/admin/api/documents/' + encodeURIComponent(cur.slug) + '/files/' + encodeURIComponent(fn), { method: 'DELETE' });
              if (r.ok && r.data.document) { cur = r.data.document; renderDocFiles(cur); toast('File deleted (change logged)'); refreshAll(false); }
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
          toast('File uploaded & change recorded');
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

    var decodeJawiBtn = document.getElementById('rf-d-decode-jawi-btn');
    if (decodeJawiBtn) {
      decodeJawiBtn.onclick = async function () {
        var jawiEl = document.getElementById('rf-d-jawi');
        var raw = jawiEl ? jawiEl.value.trim() : '';
        if (!raw) {
          setFieldError(jawiEl, 'Enter Jawi text first to decode.');
          if (jawiEl) jawiEl.focus();
          return;
        }
        decodeJawiBtn.disabled = true;
        decodeJawiBtn.textContent = '⏳ Decoding…';
        try {
          var res = await api('/canvas/api/jawi-preview', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ jawi_text: raw })
          });
          if (res.ok && res.data && res.data.standard_malay_rumi_text) {
            document.getElementById('rf-d-text').value = res.data.standard_malay_rumi_text;
            toast('Decoded Jawi → Standard BM Rumi');
          } else {
            toast((res.data && res.data.reason) || 'Decode failed', false);
          }
        } catch (e) {
          toast('Decode error: ' + e.message, false);
        } finally {
          decodeJawiBtn.disabled = false;
          decodeJawiBtn.textContent = '🔄 Decode Jawi → Rumi';
        }
      };
    }

    document.getElementById('rf-d-form').onsubmit = async function (ev) {
      ev.preventDefault();
      var dForm = ev.target;
      clearFormErrors(dForm);
      var titleEl = document.getElementById('rf-d-title');
      if (!titleEl.value.trim()) {
        setFieldError(titleEl, 'Document title / subject is required.');
        titleEl.focus();
        return;
      }
      var payload = {
        title: document.getElementById('rf-d-title').value,
        subject: document.getElementById('rf-d-title').value,
        group_slug: document.getElementById('rf-d-group').value,
        reference_code: document.getElementById('rf-d-ref').value,
        category: document.getElementById('rf-d-cat').value,
        year: document.getElementById('rf-d-year').value,
        script_type: document.getElementById('rf-d-script').value,
        date_created: document.getElementById('rf-d-date').value,
        origin_location: document.getElementById('rf-d-orig').value,
        author_or_source: document.getElementById('rf-d-auth').value,
        summary: document.getElementById('rf-d-sum').value,
        jawi_text: document.getElementById('rf-d-jawi').value,
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
      if (r.ok) {
        var savedSlug = (r.data && r.data.document && r.data.document.slug) || (doc && doc.slug);
        if (!doc && pendingInitialFile && savedSlug) {
          try {
            var fd2 = new FormData();
            fd2.append('file', pendingInitialFile);
            await api('/admin/api/documents/' + encodeURIComponent(savedSlug) + '/files?force=1', { method: 'POST', body: fd2 });
          } catch (e) {}
        }
        trackUserPref({
          event_type: 'save_document',
          category: payload.category,
          source: payload.author_or_source,
          year: payload.year,
          item_slug: savedSlug || '',
          item_title: payload.title
        });
        wrap.remove();
        toast(doc ? 'Document updated (change history recorded)' : 'Document created');
        await refreshAll(false);
      } else toast((r.data && r.data.reason) || 'Save failed', false);
    };
  }

  // ─── Maintainable Categories Manager Modal ────────────────────────────────
  function openCategoriesModal() {
    var wrap = document.createElement('div');
    wrap.className = 'rf-modal-backdrop';

    function draw() {
      var rows = state.categories.map(function (c) {
        return '<div class="rf-cat-row">' +
          '<span class="rf-cat-dot" style="background:' + esc(c.color || '#3b82f6') + '"></span>' +
          '<div style="flex:1;min-width:0"><strong>' + esc(c.name) + '</strong> <span class="rf-badge rf-badge-info">' + Number(c.document_count || 0) + ' doc(s)</span>' +
          '<div style="font-size:.76rem;color:rgb(var(--text-secondary))">' + esc(c.description || 'No description') + '</div></div>' +
          '<button type="button" class="rf-btn rf-btn-sm" data-rf-cat-edit="' + esc(c.slug) + '">' + EDIT_SVG + '<span>Edit</span></button>' +
          '<button type="button" class="rf-btn rf-btn-danger rf-btn-sm" data-rf-cat-del="' + esc(c.slug) + '">' + TRASH_SVG + '</button>' +
          '</div>';
      }).join('');

      wrap.innerHTML =
        '<div class="rf-modal"><h2>🏷️ Manage Categories</h2>' +
        '<p style="font-size:.84rem;color:rgb(var(--text-secondary));margin:-6px 0 14px">Add, rename, or remove archival categories. Renaming automatically updates matching documents.</p>' +
        '<form id="rf-cat-add-form" class="rf-cat-add-bar">' +
        '<input class="rf-input" id="rf-cat-name" placeholder="New category name (e.g. Perjanjian)" required style="flex:1;min-width:160px"/>' +
        '<input class="rf-input" id="rf-cat-desc" placeholder="Short description (optional)" style="flex:1;min-width:160px"/>' +
        '<input type="color" id="rf-cat-color" value="#3b82f6" title="Badge color" style="width:40px;height:36px;border-radius:8px;border:1px solid var(--border-medium);background:transparent;cursor:pointer"/>' +
        '<button type="submit" class="rf-btn rf-btn-primary rf-btn-sm">＋ Add</button>' +
        '</form>' +
        '<div class="rf-cat-list">' + (rows || '<div class="rf-empty">No categories yet.</div>') + '</div>' +
        '<div class="rf-actions" style="justify-content:flex-end;margin-top:14px"><button type="button" class="rf-btn" id="rf-cat-close">Done</button></div>' +
        '</div>';

      wrap.querySelector('#rf-cat-close').onclick = function () { wrap.remove(); };
      wrap.querySelector('#rf-cat-add-form').onsubmit = async function (ev) {
        ev.preventDefault();
        var r = await api('/admin/api/categories', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            name: wrap.querySelector('#rf-cat-name').value,
            description: wrap.querySelector('#rf-cat-desc').value,
            color: wrap.querySelector('#rf-cat-color').value
          })
        });
        if (r.ok) {
          toast('Category added');
          var cr = await api('/admin/api/categories?ts=' + Date.now());
          if (cr.ok && Array.isArray(cr.data)) state.categories = cr.data;
          draw();
          renderBody();
        } else toast((r.data && r.data.reason) || 'Failed adding category', false);
      };
      wrap.querySelectorAll('[data-rf-cat-edit]').forEach(function (b) {
        b.onclick = function () {
          var slug = b.getAttribute('data-rf-cat-edit');
          var cat = state.categories.find(function (x) { return x.slug === slug; });
          if (!cat) return;
          var newName = window.prompt('Rename category:', cat.name);
          if (!newName || !newName.trim()) return;
          var newDesc = window.prompt('Category description:', cat.description || '');
          api('/admin/api/categories/' + encodeURIComponent(slug), {
            method: 'PATCH',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ name: newName.trim(), description: newDesc == null ? cat.description : newDesc })
          }).then(async function (r) {
            if (r.ok) {
              toast('Category updated (' + (r.data.updated_documents || 0) + ' doc(s) updated)');
              await refreshAll(false);
              draw();
            } else toast((r.data && r.data.reason) || 'Update failed', false);
          });
        };
      });
      wrap.querySelectorAll('[data-rf-cat-del]').forEach(function (b) {
        b.onclick = function () {
          var slug = b.getAttribute('data-rf-cat-del');
          var cat = state.categories.find(function (x) { return x.slug === slug; });
          if (!cat) return;
          openConfirmModal({
            title: 'Delete Category "' + cat.name + '"?',
            message: 'Remove "' + cat.name + '"? Any ' + Number(cat.document_count || 0) + ' linked document(s) will keep all their files and be reassigned to "Lain-lain".',
            confirmLabel: 'Delete Category',
            onConfirm: async function () {
              var r = await api('/admin/api/categories/' + encodeURIComponent(slug) + '?reassign_to=lain_lain', { method: 'DELETE' });
              if (r.ok) {
                toast('Category deleted');
                await refreshAll(false);
                draw();
              } else toast((r.data && r.data.reason) || 'Delete failed', false);
            }
          });
        };
      });
    }

    document.body.appendChild(wrap);
    wrap.onclick = function (e) { if (e.target === wrap) wrap.remove(); };
    draw();
  }

  // ─── User Preferences & Personalization Modal ─────────────────────────────
  function openPreferencesModal() {
    var wrap = document.createElement('div');
    wrap.className = 'rf-modal-backdrop';
    var pref = (state.preferencesData && state.preferencesData.preferences) || {};
    var topCats = (pref.top_categories || []).map(function (x) { return '<span class="rf-badge rf-badge-info">' + esc(x.value) + ' (' + x.clicks + ')</span>'; }).join(' ');
    var topSrcs = (pref.top_sources || []).map(function (x) { return '<span class="rf-badge rf-badge-ok">' + esc(x.value) + ' (' + x.clicks + ')</span>'; }).join(' ');
    var topFlds = (pref.top_folders || []).map(function (x) { return '<span class="rf-badge">' + esc(x.value) + ' (' + x.clicks + ')</span>'; }).join(' ');

    wrap.innerHTML =
      '<div class="rf-modal"><h2>⭐ My Preferences &amp; Learned Interests</h2>' +
      '<p style="font-size:.84rem;color:rgb(var(--text-secondary));margin:-6px 0 14px">ArkibVision learns which categories, sources, and folders you click most often, and lets you pin your own preferred items.</p>' +
      '<div class="rf-pref-learned">' +
      '<div><b>Most Clicked Categories:</b> ' + (topCats || '<span style="opacity:.7">No clicks recorded yet</span>') + '</div>' +
      '<div><b>Most Clicked Sources:</b> ' + (topSrcs || '<span style="opacity:.7">No clicks recorded yet</span>') + '</div>' +
      '<div><b>Most Clicked Folders:</b> ' + (topFlds || '<span style="opacity:.7">No clicks recorded yet</span>') + '</div>' +
      '</div>' +
      '<form id="rf-pref-form" class="rf-form-grid" style="margin-top:12px">' +
      '<div class="rf-field full"><label>Pinned Preferred Categories (comma-separated)</label><input class="rf-input" id="rf-p-cats" placeholder="e.g. Manuskrip, Surat Rasmi" value="' + esc((pref.pinned_categories || []).join(', ')) + '"/></div>' +
      '<div class="rf-field full"><label>Pinned Preferred Sources (comma-separated)</label><input class="rf-input" id="rf-p-srcs" placeholder="e.g. Arkib Negara Malaysia, Jabatan Perdana Menteri" value="' + esc((pref.pinned_sources || []).join(', ')) + '"/></div>' +
      '<div class="rf-field full"><label>Pinned Preferred Folders (comma-separated)</label><input class="rf-input" id="rf-p-flds" placeholder="e.g. Historical Manuscripts" value="' + esc((pref.pinned_folders || []).join(', ')) + '"/></div>' +
      '<div class="rf-field full"><label>Custom Interest Keywords / Tags (comma-separated)</label><input class="rf-input" id="rf-p-tags" placeholder="e.g. merdeka, jawi, perjanjian, sultan" value="' + esc((pref.custom_interests || []).join(', ')) + '"/></div>' +
      '<div class="rf-actions full" style="justify-content:space-between;margin-top:10px">' +
      '<button type="button" class="rf-btn rf-btn-danger rf-btn-sm" id="rf-pref-reset">Reset Learned History</button>' +
      '<div style="display:flex;gap:8px"><button type="button" class="rf-btn" id="rf-pref-cancel">Cancel</button><button type="submit" class="rf-btn rf-btn-primary">Save Preferences</button></div>' +
      '</div></form></div>';

    document.body.appendChild(wrap);
    wrap.onclick = function (e) { if (e.target === wrap) wrap.remove(); };
    wrap.querySelector('#rf-pref-cancel').onclick = function () { wrap.remove(); };
    wrap.querySelector('#rf-pref-reset').onclick = async function () {
      var r = await api('/admin/api/preferences/reset', { method: 'POST' });
      if (r.ok) { wrap.remove(); toast('Preferences reset'); await refreshAll(false); }
    };
    wrap.querySelector('#rf-pref-form').onsubmit = async function (ev) {
      ev.preventDefault();
      var splitCsv = function (id) {
        return wrap.querySelector(id).value.split(',').map(function (s) { return s.trim(); }).filter(Boolean);
      };
      var r = await api('/admin/api/preferences', {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          pinned_categories: splitCsv('#rf-p-cats'),
          pinned_sources: splitCsv('#rf-p-srcs'),
          pinned_folders: splitCsv('#rf-p-flds'),
          custom_interests: splitCsv('#rf-p-tags')
        })
      });
      if (r.ok) { wrap.remove(); toast('Preferences saved'); await refreshAll(false); }
      else toast('Failed saving preferences', false);
    };
  }

  // ─── Before vs. After Change Detection & Revision Diff Modal ──────────────
  async function openRevisionsModal(entityType, slug, title) {
    var wrap = document.createElement('div');
    wrap.className = 'rf-modal-backdrop';
    wrap.innerHTML = '<div class="rf-modal rf-modal-wide"><h2>🕒 Change History — ' + esc(title) + '</h2><div class="rf-empty">Loading revision history…</div></div>';
    document.body.appendChild(wrap);
    wrap.onclick = function (e) { if (e.target === wrap) wrap.remove(); };

    var r = await api('/admin/api/revisions/' + encodeURIComponent(entityType) + '/' + encodeURIComponent(slug) + '?ts=' + Date.now());
    var revs = (r.ok && r.data && Array.isArray(r.data.revisions)) ? r.data.revisions : [];
    var selectedIdx = 0;

    function drawRev() {
      if (!revs.length) {
        wrap.innerHTML =
          '<div class="rf-modal"><h2>🕒 Change History — ' + esc(title) + '</h2>' +
          '<div class="rf-empty">No edits or file changes recorded for this record yet.</div>' +
          '<div class="rf-actions" style="justify-content:flex-end;margin-top:14px"><button type="button" class="rf-btn" id="rf-rev-close">Close</button></div></div>';
        wrap.querySelector('#rf-rev-close').onclick = function () { wrap.remove(); };
        return;
      }
      var cur = revs[selectedIdx] || revs[0];
      var listHtml = revs.map(function (rv, idx) {
        return '<button type="button" class="rf-rev-item' + (idx === selectedIdx ? ' active' : '') + '" data-rf-rev-idx="' + idx + '">' +
          '<div class="rf-rev-item-title">' + esc(rv.summary || rv.action) + '</div>' +
          '<div class="rf-rev-item-meta">' + esc(rv.actor || 'admin') + ' · ' + esc((rv.timestamp || '').replace('T', ' ').slice(0, 19)) + '</div>' +
          '</button>';
      }).join('');

      var detailHtml = '';
      if (cur.field_diffs && cur.field_diffs.length) {
        detailHtml += '<div class="rf-diff-section"><h4>Metadata &amp; Text Changes</h4>' +
          cur.field_diffs.map(function (fd) {
            return '<div class="rf-diff-card">' +
              '<div class="rf-diff-field-head"><strong>' + esc(fd.label || fd.field) + '</strong><span class="rf-badge ' + (fd.kind === 'added' ? 'rf-badge-ok' : fd.kind === 'removed' ? 'rf-badge-crit' : 'rf-badge-warn') + '">' + esc(fd.kind) + '</span></div>' +
              '<div class="rf-diff-cols">' +
              '<div class="rf-diff-before"><div class="rf-diff-lbl">Before</div><pre>' + esc(fd.before || '(empty)') + '</pre></div>' +
              '<div class="rf-diff-after"><div class="rf-diff-lbl">After</div><pre>' + esc(fd.after || '(empty)') + '</pre></div>' +
              '</div></div>';
          }).join('') + '</div>';
      }
      if (cur.file_diff) {
        var fd = cur.file_diff;
        detailHtml += '<div class="rf-diff-section"><h4>File / Image Change — ' + esc(fd.filename) + ' (' + esc(fd.change_kind) + ')</h4>' +
          '<div class="rf-diff-cols">' +
          '<div class="rf-diff-before"><div class="rf-diff-lbl">Before</div>' +
          (fd.is_image && fd.before_snapshot_url ? '<img class="rf-diff-img" src="' + esc(apiUrl(fd.before_snapshot_url)) + '" alt="Before"/>' : '<div class="rf-diff-placeholder">' + (fd.before_snapshot_url ? '📄 Previous file snapshot saved' : '(did not exist)') + '</div>') +
          '<div class="rf-diff-meta">' +
          (fd.before_dims ? 'Dimensions: ' + esc(fd.before_dims) + '<br/>' : '') +
          (fd.before_size != null ? 'Size: ' + fd.before_size + ' bytes<br/>' : '') +
          (fd.before_sha256 ? 'SHA-256: <code>' + esc(fd.before_sha256.slice(0, 16)) + '…</code>' : '') +
          '</div></div>' +
          '<div class="rf-diff-after"><div class="rf-diff-lbl">After</div>' +
          (fd.is_image && fd.after_snapshot_url ? '<img class="rf-diff-img" src="' + esc(apiUrl(fd.after_snapshot_url)) + '" alt="After"/>' : '<div class="rf-diff-placeholder">' + (fd.after_snapshot_url ? '📄 Updated file snapshot saved' : '(removed)') + '</div>') +
          '<div class="rf-diff-meta">' +
          (fd.after_dims ? 'Dimensions: ' + esc(fd.after_dims) + '<br/>' : '') +
          (fd.after_size != null ? 'Size: ' + fd.after_size + ' bytes<br/>' : '') +
          (fd.after_sha256 ? 'SHA-256: <code>' + esc(fd.after_sha256.slice(0, 16)) + '…</code>' : '') +
          '</div></div></div>' +
          (fd.perceptual_distance != null ? '<div style="margin-top:8px;font-size:.82rem"><b>Visual Perceptual Difference (aHash):</b> ' + fd.perceptual_distance + ' / 64 bits changed</div>' : '') +
          '</div>';
      }

      wrap.innerHTML =
        '<div class="rf-modal rf-modal-wide">' +
        '<div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:12px"><div><h2 style="margin:0">🕒 Before vs. After Changes — ' + esc(title) + '</h2><div style="font-size:.82rem;color:rgb(var(--text-secondary))">' + revs.length + ' recorded revision(s)</div></div><button type="button" class="rf-btn rf-btn-sm" id="rf-rev-close">✕ Close</button></div>' +
        '<div class="rf-rev-layout"><div class="rf-rev-list">' + listHtml + '</div><div class="rf-rev-detail">' + detailHtml + '</div></div>' +
        '</div>';

      wrap.querySelector('#rf-rev-close').onclick = function () { wrap.remove(); };
      wrap.querySelectorAll('[data-rf-rev-idx]').forEach(function (b) {
        b.onclick = function () { selectedIdx = Number(b.getAttribute('data-rf-rev-idx')); drawRev(); };
      });
    }
    drawRev();
  }

  // ─── External SQL Audit Log Inspector Modal ───────────────────────────────
  function openSqlInspectModal(onDone) {
    var wrap = document.createElement('div');
    wrap.className = 'rf-modal-backdrop';
    wrap.innerHTML =
      '<div class="rf-modal"><h2>🗄️ Inspect External SQL / Database Audit Logs</h2>' +
      '<p style="font-size:.84rem;color:rgb(var(--text-secondary));margin:-6px 0 12px">Paste raw SQL queries or JSONL audit lines to scan for SQL injection, destructive queries missing WHERE, DDL schema changes, or unbounded SELECT * dumps.</p>' +
      '<div class="rf-field"><label>Actor / DB User (optional)</label><input class="rf-input" id="rf-sql-actor" value="external_db_user"/></div>' +
      '<div class="rf-field" style="margin-top:10px"><label>SQL Queries or Audit Log Lines</label><textarea class="rf-textarea" id="rf-sql-input" style="min-height:150px;font-family:monospace" placeholder="SELECT * FROM archival_profiles;\nDELETE FROM documents;\nSELECT * FROM users WHERE name = \'\' OR 1=1 UNION SELECT * FROM secrets--"></textarea></div>' +
      '<div id="rf-sql-out" style="margin-top:10px"></div>' +
      '<div class="rf-actions" style="justify-content:flex-end;margin-top:14px">' +
      '<button type="button" class="rf-btn" id="rf-sql-cancel">Close</button>' +
      '<button type="button" class="rf-btn rf-btn-primary" id="rf-sql-run">🔍 Run Rules Engine</button>' +
      '</div></div>';

    document.body.appendChild(wrap);
    wrap.onclick = function (e) { if (e.target === wrap) wrap.remove(); };
    wrap.querySelector('#rf-sql-cancel').onclick = function () { wrap.remove(); };
    wrap.querySelector('#rf-sql-run').onclick = async function () {
      var sqlText = wrap.querySelector('#rf-sql-input').value;
      var actor = wrap.querySelector('#rf-sql-actor').value;
      var r = await api('/admin/api/audit/inspect-sql', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ sql_log: sqlText, actor: actor })
      });
      if (!r.ok) {
        toast((r.data && r.data.reason) || 'Inspection failed', false);
        return;
      }
      state.sqlInspectResult = r.data;
      var alerts = r.data.alerts || [];
      wrap.querySelector('#rf-sql-out').innerHTML =
        '<div class="rf-audit-summary-box"><b>Scanned ' + Number(r.data.lines_analyzed || 0) + ' line(s) · Found ' + alerts.length + ' anomaly(ies) · Risk: ' + esc(r.data.risk_level) + '</b>' +
        (alerts.length ? '<ul style="margin:8px 0 0;padding-left:18px">' + alerts.map(function (a) {
          return '<li><span class="rf-badge ' + (a.severity === 'CRITICAL' || a.severity === 'HIGH' ? 'rf-badge-crit' : 'rf-badge-warn') + '">' + esc(a.severity) + '</span> <b>' + esc(a.title) + '</b> — ' + esc(a.recommended_action) + '</li>';
        }).join('') + '</ul>' : '<div style="margin-top:6px;color:#10b981">✓ No abnormal SQL patterns found.</div>') +
        '</div>';
      if (onDone) onDone();
    };
  }

  function humanAuditDescription(ev) {
    var p = String(ev.path || '');
    var a = String(ev.action || '');
    if (a === 'LOGIN_OK') return 'Signed in to Admin Portal';
    if (a === 'LOGIN_FAIL') return 'Failed Admin sign-in attempt';
    if (p.indexOf('/audit/report/') !== -1) return 'Downloaded Audit Report (' + p.split('/').pop().toUpperCase() + ')';
    if (p.indexOf('/audit/inspect-sql') !== -1) return 'Ran SQL Audit Log Inspection';
    if (p.indexOf('/audit/explain') !== -1) return 'Generated AI Anomaly Explanation';
    if (p.indexOf('/revisions/scan-disk') !== -1) return 'Ran External On-Disk File Change Scan';
    if (p.indexOf('/analyze-upload') !== -1) return 'Analyzed uploaded file with AI Auto-Fill';
    if (p.indexOf('/export/') !== -1) return 'Exported document (' + p.split('/').pop().toUpperCase() + ')';
    if (p.indexOf('/categories') !== -1) {
      return a === 'CREATE' ? 'Added a new category' : a === 'DELETE' ? 'Deleted a category' : a === 'UPDATE' ? 'Updated category' : 'Viewed categories';
    }
    if (p.indexOf('/documents') !== -1) {
      if (p.indexOf('/files') !== -1) return a === 'DELETE' ? 'Removed file from document' : 'Uploaded file to document';
      return a === 'CREATE' ? 'Created archival document' : a === 'DELETE' ? 'Deleted archival document' : a === 'UPDATE' ? 'Edited archival document' : 'Browsed archival documents';
    }
    if (p.indexOf('/profiles') !== -1) {
      if (p.indexOf('/images') !== -1) return a === 'DELETE' ? 'Removed person photo' : 'Added/updated person photo';
      return a === 'CREATE' ? 'Enrolled new person' : a === 'DELETE' ? 'Deleted person profile' : a === 'UPDATE' ? 'Edited person profile' : 'Browsed enrolled people';
    }
    if (p.indexOf('/locations') !== -1) {
      return a === 'CREATE' ? 'Created archival location' : a === 'DELETE' ? 'Deleted archival location' : a === 'UPDATE' ? 'Edited archival location' : 'Browsed archival locations';
    }
    return ev.method + ' ' + p;
  }

  function changeActionBadge(action) {
    var a = String(action || '').toLowerCase();
    if (a === 'created') return '<span class="rf-badge rf-badge-ok">＋ CREATED</span>';
    if (a === 'file_added') return '<span class="rf-badge rf-badge-ok">＋ FILE ADDED</span>';
    if (a === 'deleted' || a === 'file_deleted') return '<span class="rf-badge rf-badge-crit">✕ DELETED</span>';
    if (a === 'external_modified') return '<span class="rf-badge rf-badge-crit">⚠ DISK CHANGED</span>';
    if (a === 'image_edited' || a === 'file_replaced') return '<span class="rf-badge rf-badge-warn">✎ FILE REPLACED</span>';
    return '<span class="rf-badge rf-badge-warn">✎ UPDATED</span>';
  }

  // ─── Tab 5: User Behavior Insights, Search History & Hot-Topics Graphs ────
  function renderInsights(body) {
    var bd = state.behaviorData || {};
    var sum = bd.summary || { total_events: 0, chatbot_questions: 0, search_queries: 0, item_checks: 0, hot_topic_count: 0 };
    var topics = Array.isArray(bd.hot_topics) ? bd.hot_topics : [];
    var questions = Array.isArray(bd.top_chatbot_questions) ? bd.top_chatbot_questions : [];
    var searches = Array.isArray(bd.top_searches) ? bd.top_searches : [];
    var checked = Array.isArray(bd.top_checked_items) ? bd.top_checked_items : [];
    var tools = Array.isArray(bd.tool_usage) ? bd.tool_usage : [];
    var cats = Array.isArray(bd.category_interest) ? bd.category_interest : [];
    var trend = Array.isArray(bd.daily_trend) ? bd.daily_trend : [];
    var insights = Array.isArray(bd.admin_insights) ? bd.admin_insights : [];
    var history = Array.isArray(bd.recent_history) ? bd.recent_history : [];

    // 1. Pure-SVG 7-Day Multi-Series Activity Trend Chart
    var trendSvg = (function () {
      var W = 580, H = 200, padL = 34, padR = 16, padT = 18, padB = 30;
      var plotW = W - padL - padR, plotH = H - padT - padB;
      var maxY = 4;
      trend.forEach(function (d) {
        maxY = Math.max(maxY, Number(d.chatbot || 0), Number(d.searches || 0), Number(d.item_views || 0));
      });
      var n = Math.max(trend.length, 2);
      var xPos = function (i) { return padL + (plotW * i) / (n - 1); };
      var yPos = function (v) { return padT + plotH - (plotH * v) / maxY; };
      var linePath = function (key) {
        return trend.map(function (d, i) {
          return (i === 0 ? 'M' : 'L') + xPos(i).toFixed(1) + ',' + yPos(Number(d[key] || 0)).toFixed(1);
        }).join(' ');
      };
      var areaPath = function (key) {
        if (!trend.length) return '';
        return linePath(key) + ' L' + xPos(trend.length - 1).toFixed(1) + ',' + (padT + plotH) + ' L' + xPos(0).toFixed(1) + ',' + (padT + plotH) + ' Z';
      };
      var grid = [0, 0.25, 0.5, 0.75, 1].map(function (f) {
        var y = padT + plotH * (1 - f);
        var val = Math.round(maxY * f);
        return '<line x1="' + padL + '" y1="' + y.toFixed(1) + '" x2="' + (W - padR) + '" y2="' + y.toFixed(1) + '" stroke="rgba(148,163,184,0.18)" stroke-dasharray="3,3"/>' +
          '<text x="' + (padL - 6) + '" y="' + (y + 3).toFixed(1) + '" text-anchor="end" fill="#94a3b8" font-size="10">' + val + '</text>';
      }).join('');
      var xLabels = trend.map(function (d, i) {
        return '<text x="' + xPos(i).toFixed(1) + '" y="' + (H - 8) + '" text-anchor="middle" fill="#94a3b8" font-size="10">' + esc(d.label || d.date) + '</text>';
      }).join('');
      var dots = function (key, color) {
        return trend.map(function (d, i) {
          var v = Number(d[key] || 0);
          return '<circle cx="' + xPos(i).toFixed(1) + '" cy="' + yPos(v).toFixed(1) + '" r="3.5" fill="' + color + '"><title>' + esc(d.label) + ': ' + v + '</title></circle>';
        }).join('');
      };
      return '<svg viewBox="0 0 ' + W + ' ' + H + '" class="rf-svg-chart" role="img" aria-label="7-day user activity trend">' +
        grid + xLabels +
        (trend.length ? '<path d="' + areaPath('chatbot') + '" fill="rgba(59,130,246,0.16)"/>' : '') +
        (trend.length ? '<path d="' + linePath('chatbot') + '" fill="none" stroke="#3b82f6" stroke-width="2.4"/>' : '') +
        (trend.length ? '<path d="' + linePath('searches') + '" fill="none" stroke="#f59e0b" stroke-width="2.2"/>' : '') +
        (trend.length ? '<path d="' + linePath('item_views') + '" fill="none" stroke="#10b981" stroke-width="2.2"/>' : '') +
        dots('chatbot', '#3b82f6') + dots('searches', '#f59e0b') + dots('item_views', '#10b981') +
        '</svg>';
    })();

    // 2. Pure-SVG Capability Donut Chart
    var palette = ['#3b82f6', '#8b5cf6', '#10b981', '#f59e0b', '#ec4899', '#06b6d4', '#64748b'];
    var donutHtml = (function () {
      var list = tools.length ? tools.slice(0, 7) : cats.slice(0, 7);
      var total = list.reduce(function (acc, x) { return acc + Number(x.count || 0); }, 0);
      if (!total) return '<div class="rf-empty" style="padding:28px 12px">Ask the chatbot or browse records to see interest distribution.</div>';
      var r = 56, cx = 74, cy = 74, circ = 2 * Math.PI * r, offset = 0;
      var slices = list.map(function (item, idx) {
        var frac = Number(item.count || 0) / total;
        var len = frac * circ;
        var color = palette[idx % palette.length];
        var seg = '<circle r="' + r + '" cx="' + cx + '" cy="' + cy + '" fill="transparent" stroke="' + color + '" stroke-width="20" stroke-dasharray="' + len.toFixed(2) + ' ' + (circ - len).toFixed(2) + '" stroke-dashoffset="' + (-offset).toFixed(2) + '" transform="rotate(-90 ' + cx + ' ' + cy + ')"><title>' + esc(item.name) + ': ' + item.count + '</title></circle>';
        offset += len;
        return seg;
      }).join('');
      var legend = list.map(function (item, idx) {
        var pct = Math.round((Number(item.count || 0) * 100) / total);
        return '<div class="rf-donut-leg-row"><span class="rf-cat-dot" style="background:' + palette[idx % palette.length] + '"></span><span style="flex:1">' + esc(item.name) + '</span><b>' + item.count + '</b> <span style="opacity:.7">(' + pct + '%)</span></div>';
      }).join('');
      return '<div class="rf-donut-wrap">' +
        '<svg viewBox="0 0 148 148" width="148" height="148">' + slices +
        '<text x="74" y="71" text-anchor="middle" fill="currentColor" font-size="18" font-weight="700">' + total + '</text>' +
        '<text x="74" y="87" text-anchor="middle" fill="#94a3b8" font-size="9">INTERACTIONS</text></svg>' +
        '<div class="rf-donut-legend">' + legend + '</div></div>';
    })();

    // 3. Hot Topics Horizontal Bar Chart
    var maxTopicScore = topics.length ? Math.max.apply(null, topics.map(function (t) { return Number(t.score || t.count || 1); })) : 1;
    var hotTopicsHtml = topics.length ? topics.slice(0, 10).map(function (t, idx) {
      var pct = Math.max(8, Math.round((Number(t.score || t.count || 0) / maxTopicScore) * 100));
      var barColor = palette[idx % palette.length];
      return '<div class="rf-bar-row">' +
        '<div class="rf-bar-lbl"><strong>' + esc(t.topic) + '</strong>' +
        (Number(t.recent_24h || 0) > 0 ? ' <span class="rf-badge rf-badge-warn">🔥 ' + t.recent_24h + ' today</span>' : '') + '</div>' +
        '<div class="rf-bar-track"><div class="rf-bar-fill" style="width:' + pct + '%;background:' + barColor + '"></div></div>' +
        '<div class="rf-bar-val">' + Number(t.count || 0) + '×</div>' +
        '</div>';
    }).join('') : '<div class="rf-empty" style="padding:28px 12px">No hot topics captured yet. Try asking questions in the chatbot or searching in Documents/People.</div>';

    var insightCards = Array.isArray(bd.admin_insight_cards) && bd.admin_insight_cards.length
      ? bd.admin_insight_cards
      : insights.map(function (ins) {
        return { icon: '💡', tag: 'INSIGHT', tone: 'info', headline: 'User Activity Insight', metric: '', detail: ins };
      });

    var insightCardsHtml =
      '<div class="rf-insight-grid">' +
      insightCards.map(function (c) {
        var bCls = c.tone === 'warn' ? 'rf-badge-warn' : c.tone === 'ok' ? 'rf-badge-ok' : 'rf-badge-info';
        return '<div class="rf-insight-card rf-insight-' + esc(c.tone || 'info') + '">' +
          '<div class="rf-insight-top">' +
          '<span class="rf-badge ' + bCls + '">' + esc(c.icon || '💡') + ' ' + esc(c.tag || 'INSIGHT') + '</span>' +
          (c.metric ? '<span class="rf-insight-metric">' + esc(c.metric) + '</span>' : '') +
          '</div>' +
          '<div class="rf-insight-head">' + esc(c.headline || '') + '</div>' +
          '<div class="rf-insight-desc">' + esc(c.detail || '') + '</div>' +
          '</div>';
      }).join('') +
      '</div>';

    body.innerHTML =
      '<div class="rf-section-head"><div><h2>📈 User Behavior, Search History &amp; Hot Topics</h2>' +
      '<p>Automatically analyzes what users ask in the chatbot, what they search, and which records they check most frequently — ranked with recency-weighted scoring.</p></div>' +
      '<div style="display:flex;gap:8px;flex-wrap:wrap">' +
      '<button type="button" class="rf-btn rf-btn-primary" id="rf-ins-refresh">↻ Refresh Analytics</button>' +
      '</div></div>' +
      '<div class="rf-kpi-grid">' +
      '<div class="rf-kpi-card"><div class="rf-kpi-lbl">Total User Signals</div><div class="rf-kpi-val">' + Number(sum.total_events || 0) + '</div></div>' +
      '<div class="rf-kpi-card"><div class="rf-kpi-lbl">Chatbot Questions</div><div class="rf-kpi-val" style="color:#60a5fa">' + Number(sum.chatbot_questions || 0) + '</div></div>' +
      '<div class="rf-kpi-card"><div class="rf-kpi-lbl">Admin Searches</div><div class="rf-kpi-val" style="color:#fbbf24">' + Number(sum.search_queries || 0) + '</div></div>' +
      '<div class="rf-kpi-card"><div class="rf-kpi-lbl">Records Checked</div><div class="rf-kpi-val" style="color:#34d399">' + Number(sum.item_checks || 0) + '</div></div>' +
      '<div class="rf-kpi-card"><div class="rf-kpi-lbl">Hot Topics Detected</div><div class="rf-kpi-val">' + Number(sum.hot_topic_count || 0) + '</div></div>' +
      '</div>' +
      '<div class="rf-audit-panel"><h3>💡 Actionable Admin Insights</h3>' + insightCardsHtml + '</div>' +
      '<div class="rf-chart-grid">' +
      '<div class="rf-audit-panel"><h3>🔥 Hot Topics &amp; Trending Keywords</h3><div class="rf-bar-chart">' + hotTopicsHtml + '</div></div>' +
      '<div class="rf-audit-panel"><h3>📈 7-Day User Activity Trend</h3>' +
      '<div class="rf-chart-legend"><span><i style="background:#3b82f6"></i> Chatbot Questions</span><span><i style="background:#f59e0b"></i> Searches</span><span><i style="background:#10b981"></i> Records Checked</span></div>' +
      trendSvg + '</div>' +
      '<div class="rf-audit-panel"><h3>🍩 Most Used AI Features &amp; Interests</h3>' + donutHtml + '</div>' +
      '<div class="rf-audit-panel"><h3>💬 Frequently Asked in Chatbot</h3>' +
      (questions.length ? questions.slice(0, 8).map(function (q) {
        return '<div class="rf-alert-row"><div style="display:flex;justify-content:space-between;gap:8px;align-items:flex-start"><strong>“' + esc(q.question) + '”</strong><span class="rf-badge rf-badge-info">' + Number(q.count || 1) + '×</span></div>' +
          '<div style="font-size:.75rem;color:rgb(var(--text-secondary));margin-top:4px">' + esc(q.feature || 'Chatbot') + ' · ' + esc(fmtTime(q.last_seen)) + '</div></div>';
      }).join('') : '<div style="font-size:.84rem;color:rgb(var(--text-secondary))">No chatbot questions logged yet. Send any question in the LibreChat chatbox to track it here.</div>') +
      '</div></div>' +
      '<div class="rf-chart-grid">' +
      '<div class="rf-audit-panel"><h3>🔍 Top &amp; Recent Search Queries</h3>' +
      (searches.length ? searches.slice(0, 10).map(function (s) {
        return '<div class="rf-alert-row" style="display:flex;justify-content:space-between;align-items:center;gap:8px"><div><strong>' + esc(s.query) + '</strong> <span class="rf-badge">' + esc(s.channel) + '</span><div style="font-size:.74rem;color:rgb(var(--text-secondary))">' + esc(fmtTime(s.last_seen)) + '</div></div><span class="rf-badge rf-badge-warn">' + Number(s.count || 1) + '×</span></div>';
      }).join('') : '<div style="font-size:.84rem;color:rgb(var(--text-secondary))">No search history recorded yet.</div>') +
      '</div>' +
      '<div class="rf-audit-panel"><h3>⭐ Most Frequently Checked Records</h3>' +
      (checked.length ? checked.slice(0, 10).map(function (it) {
        return '<div class="rf-alert-row" style="display:flex;justify-content:space-between;align-items:center;gap:8px"><div><span class="rf-badge rf-badge-info">' + esc(it.entity_type) + '</span> <strong>' + esc(it.title) + '</strong>' + (it.category ? ' <span class="rf-badge">' + esc(it.category) + '</span>' : '') + '<div style="font-size:.74rem;color:rgb(var(--text-secondary))">Last checked ' + esc(fmtTime(it.last_seen)) + '</div></div><span class="rf-badge rf-badge-ok">' + Number(it.count || 1) + ' views</span></div>';
      }).join('') : '<div style="font-size:.84rem;color:rgb(var(--text-secondary))">No frequently checked records yet.</div>') +
      '</div></div>' +
      '<div class="rf-audit-panel"><h3>🕒 Recent User Search &amp; Chatbot History (' + history.length + ')</h3>' +
      '<div style="overflow-x:auto"><table class="rf-audit-table"><thead><tr><th>When</th><th>Channel</th><th>Topic / Query / Item</th><th>Feature</th></tr></thead><tbody>' +
      (history.length ? history.slice(0, 20).map(function (h) {
        var chLbl = h.source === 'chatbot_prompt' ? '💬 Chatbot Question'
          : h.source === 'chatbot_tool' ? '🤖 Chatbot Tool (' + (h.tool_name || 'AI') + ')'
          : h.source === 'admin_search' ? '🔍 Admin Search'
          : '👁️ Record Check';
        var text = h.query_text || h.item_title || h.item_slug || '—';
        return '<tr><td style="white-space:nowrap">' + esc(fmtTime(h.timestamp)) + '</td><td><span class="rf-badge">' + esc(chLbl) + '</span></td><td>' + esc(text) + '</td><td>' + esc(h.feature_group || '—') + '</td></tr>';
      }).join('') : '<tr><td colspan="4" style="text-align:center;color:rgb(var(--text-secondary))">No user search or chatbot history recorded yet.</td></tr>') +
      '</tbody></table></div></div>';

    document.getElementById('rf-ins-refresh').onclick = function () { refreshAll(true); };
  }

  // ─── Tab 6: Audit Management, Anomaly Detection & Change Log ──────────────
  function renderAudit(body) {
    var data = state.auditData || {};
    var snap = data.snapshot || {
      risk_level: 'LOW',
      risk_score: 0,
      total_events_analyzed: 0,
      severity_counts: {},
      rule_alerts: [],
      statistical_outliers: [],
      recent_events: []
    };
    var allRevs = data.recent_revisions || [];
    var filteredRevs = allRevs.filter(function (rv) {
      var act = String(rv.action || '').toLowerCase();
      if (state.auditChangeFilter === 'created') return act === 'created' || act === 'file_added';
      if (state.auditChangeFilter === 'updated') return act === 'metadata_edit' || act === 'canvas_ocr_edit' || act === 'image_edited' || act === 'file_replaced';
      if (state.auditChangeFilter === 'deleted') return act === 'deleted' || act === 'file_deleted';
      return true;
    });
    var sevCls = function (s) {
      return s === 'CRITICAL' || s === 'HIGH' ? 'rf-badge-crit' : s === 'MEDIUM' ? 'rf-badge-warn' : 'rf-badge-ok';
    };

    var alertCount = (snap.rule_alerts ? snap.rule_alerts.length : 0);
    var outlierCount = (snap.statistical_outliers ? snap.statistical_outliers.length : 0);
    var calloutTone = snap.risk_level === 'CRITICAL' || snap.risk_level === 'HIGH'
      ? 'rf-callout-crit'
      : snap.risk_level === 'MEDIUM'
        ? 'rf-callout-warn'
        : 'rf-callout-ok';
    var statusHeadline = (alertCount === 0 && outlierCount === 0)
      ? '✅ All Clear — No Security or Access Anomalies Detected'
      : '⚠️ ' + (alertCount + outlierCount) + ' Anomaly Finding(s) Require Admin Review';

    var sourceLabel = state.auditAiSource
      ? (String(state.auditAiSource).toLowerCase().indexOf('fallback') !== -1
        ? '🛡️ Rules & Statistical Engine'
        : '✨ Local AI Analysis')
      : '';

    function fmtDurHuman(msVal) {
      var n = Number(msVal || 0);
      if (n >= 1000) return (n / 1000).toFixed(1) + ' s';
      return Math.round(n) + ' ms';
    }

    function formatOutlierText(raw) {
      return String(raw || '').replace(/(\d{4,})\s*ms\b/g, function (_, num) {
        return fmtDurHuman(Number(num));
      });
    }

    function renderStructuredAnomalyBody() {
      var parts = [];
      var rules = Array.isArray(snap.rule_alerts) ? snap.rule_alerts : [];
      var outliers = Array.isArray(snap.statistical_outliers) ? snap.statistical_outliers : [];

      parts.push(
        '<div class="rf-aud-meta-strip">' +
        '<span><b>Risk Level:</b> <span class="rf-badge ' + sevCls(snap.risk_level) + '">' + esc(snap.risk_level) + ' · ' + Number(snap.risk_score || 0) + '/100</span></span>' +
        '<span><b>Scope:</b> ' + Number(snap.total_events_analyzed || 0) + ' analyzed events</span>' +
        '<span><b>Last Checked:</b> ' + esc(fmtTime(snap.generated_at)) + '</span>' +
        '</div>'
      );

      if (rules.length) {
        parts.push(
          '<div class="rf-aud-sec">' +
          '<div class="rf-aud-sec-head">🚨 Security &amp; Access Rules Flagged (' + rules.length + ')</div>' +
          '<div class="rf-aud-card-list">' +
          rules.map(function (a) {
            return '<div class="rf-aud-finding-card rf-aud-sev-' + esc(String(a.severity || 'MEDIUM').toLowerCase()) + '">' +
              '<div class="rf-aud-finding-top">' +
              '<span class="rf-badge ' + sevCls(a.severity) + '">' + esc(a.severity) + '</span>' +
              '<strong class="rf-aud-finding-title">' + esc(a.title || a.rule_id) + '</strong>' +
              '<code class="rf-aud-code">' + esc(a.rule_id || '') + '</code>' +
              (a.actor || a.timestamp ? '<span class="rf-aud-finding-meta">' + esc(a.actor || '') + (a.timestamp ? ' · ' + esc(fmtTime(a.timestamp)) : '') + '</span>' : '') +
              '</div>' +
              '<p class="rf-aud-finding-desc">' + esc(a.description || '') + '</p>' +
              (a.recommended_action
                ? '<div class="rf-aud-rec"><span>💡 <b>Recommended Action:</b> ' + esc(a.recommended_action) + '</span></div>'
                : '') +
              '</div>';
          }).join('') +
          '</div></div>'
        );
      }

      if (outliers.length) {
        // Group identical outlier explanations/endpoints so repeated spikes read cleanly with a count badge
        var grouped = [];
        var gMap = {};
        outliers.forEach(function (o) {
          var key = String(o.metric || '') + '|' + String(o.severity || '') + '|' + String(o.explanation || '').replace(/\d+(\.\d+)?\s*ms/g, 'MS');
          if (!gMap[key]) {
            gMap[key] = { item: o, count: 1, maxVal: Number(o.observed_value || 0), maxZ: Number(o.z_score || 0) };
            grouped.push(gMap[key]);
          } else {
            gMap[key].count += 1;
            gMap[key].maxVal = Math.max(gMap[key].maxVal, Number(o.observed_value || 0));
            gMap[key].maxZ = Math.max(gMap[key].maxZ, Number(o.z_score || 0));
          }
        });

        parts.push(
          '<div class="rf-aud-sec">' +
          '<div class="rf-aud-sec-head">📈 Behavioral &amp; Latency Outliers (' + outliers.length + ')</div>' +
          '<div class="rf-aud-card-list">' +
          grouped.map(function (g) {
            var o = g.item;
            var prettyExp = formatOutlierText(o.explanation || '');
            var epMatch = prettyExp.match(/'([^']+)'/);
            var epBadge = epMatch ? '<code class="rf-aud-code">' + esc(epMatch[1]) + '</code>' : '';
            var metricLabel = String(o.metric || 'latency_outlier').replace(/_/g, ' ');
            return '<div class="rf-aud-finding-card rf-aud-sev-' + esc(String(o.severity || 'MEDIUM').toLowerCase()) + '">' +
              '<div class="rf-aud-finding-top">' +
              '<span class="rf-badge ' + sevCls(o.severity) + '">' + esc(o.severity) + '</span>' +
              '<strong class="rf-aud-finding-title" style="text-transform:capitalize">' + esc(metricLabel) + '</strong>' +
              epBadge +
              '<span class="rf-badge rf-badge-info">Z = ' + Number(g.maxZ || o.z_score || 0).toFixed(2) + '</span>' +
              (g.count > 1 ? '<span class="rf-badge rf-badge-warn">' + g.count + '× occurrences</span>' : '') +
              '</div>' +
              '<p class="rf-aud-finding-desc">' + esc(prettyExp) + '</p>' +
              '</div>';
          }).join('') +
          '</div></div>'
        );
      }

      // If real Ollama LLM prose was generated (not deterministic_fallback), show formatted AI summary too
      if (state.auditAiExplanation && String(state.auditAiSource || '').toLowerCase().indexOf('fallback') === -1) {
        var lines = String(state.auditAiExplanation).split(/\r?\n/).map(function (ln) { return ln.trim(); }).filter(Boolean);
        parts.push(
          '<div class="rf-aud-sec">' +
          '<div class="rf-aud-sec-head">✨ AI Executive Assessment</div>' +
          '<div class="rf-aud-ai-prose">' +
          lines.map(function (ln) {
            if (ln.indexOf('- ') === 0 || ln.indexOf('* ') === 0) {
              return '<div class="rf-aud-ai-bullet">• ' + esc(ln.slice(2)) + '</div>';
            }
            return '<p>' + esc(ln) + '</p>';
          }).join('') +
          '</div></div>'
        );
      }

      return '<div class="rf-aud-review-wrap">' + parts.join('') + '</div>';
    }

    var summaryBodyHtml = (state.auditAiExplanation || alertCount > 0 || outlierCount > 0)
      ? renderStructuredAnomalyBody()
      : '<div class="rf-sum-grid">' +
        '<div class="rf-sum-cell"><span class="rf-sum-lbl">Last Checked</span><strong>' + esc(fmtTime(snap.generated_at)) + '</strong></div>' +
        '<div class="rf-sum-cell"><span class="rf-sum-lbl">Activity Monitored</span><strong>' + Number(snap.total_events_analyzed || 0) + ' event(s)</strong></div>' +
        '<div class="rf-sum-cell"><span class="rf-sum-lbl">Security Posture</span><strong><span class="rf-badge ' + sevCls(snap.risk_level) + '">' + esc(snap.risk_level) + '</span> ' + Number(snap.risk_score || 0) + '/100</strong></div>' +
        '<div class="rf-sum-cell"><span class="rf-sum-lbl">Active Protections</span><strong>CRUD Bursts · Auth · SQLi · Z-Score Outliers</strong></div>' +
        '</div>';

    function resolveRevLocation(rv) {
      var et = String(rv.entity_type || '').toLowerCase();
      var sl = String(rv.slug || '');
      if (et === 'document') {
        var d = state.documents.find(function (x) { return x.slug === sl; });
        if (d) {
          var folder = d.group || 'Unassigned';
          return { exists: true, label: '📍 Documents › ' + folder + (d.category ? ' · ' + d.category : ''), tab: 'documents', item: d };
        }
        return { exists: false, label: '🗑️ Removed — no longer in Documents' };
      }
      if (et === 'profile') {
        var p = state.profiles.find(function (x) { return x.slug === sl; });
        if (p) {
          var pf = p.group || 'Unassigned';
          return { exists: true, label: '📍 People › ' + pf, tab: 'people', item: p };
        }
        return { exists: false, label: '🗑️ Removed — no longer in People' };
      }
      if (et === 'location') {
        var l = state.locations.find(function (x) { return x.slug === sl; });
        if (l) {
          var lf = l.group || 'Unassigned';
          return { exists: true, label: '📍 Locations › ' + lf, tab: 'locations', item: l };
        }
        return { exists: false, label: '🗑️ Removed — no longer in Locations' };
      }
      if (et === 'category') {
        var c = state.categories.find(function (x) { return x.slug === sl; });
        if (c) return { exists: true, label: '🏷️ Documents › Manage Categories', tab: 'categories', item: c };
        return { exists: false, label: '🗑️ Removed from Categories' };
      }
      return { exists: false, label: 'Archive log' };
    }

    var diskHtml = '';
    if (state.diskScanResult) {
      var ds = state.diskScanResult;
      diskHtml = '<div class="rf-audit-panel"><h3>💾 External On-Disk File Change Scan</h3>' +
        '<div style="font-size:.84rem;margin-bottom:8px">Checked <b>' + Number(ds.checked_files || 0) + '</b> registered files · Modified externally: <b>' + Number(ds.modified_count || 0) + '</b> · Missing: <b>' + Number(ds.missing_count || 0) + '</b></div>' +
        ((ds.findings && ds.findings.length)
          ? ds.findings.map(function (f) {
            return '<div class="rf-alert-row"><span class="rf-badge rf-badge-crit">' + esc(f.kind) + '</span> <b>' + esc(f.label || f.path) + '</b> <code>' + esc(f.path) + '</code></div>';
          }).join('')
          : '<div style="font-size:.84rem;color:#10b981">✓ All registered archival files match their verified SHA-256 checksums on disk.</div>') +
        '</div>';
    }

    var cntCreated = allRevs.filter(function (rv) { return rv.action === 'created' || rv.action === 'file_added'; }).length;
    var cntUpdated = allRevs.filter(function (rv) { return rv.action === 'metadata_edit' || rv.action === 'canvas_ocr_edit' || rv.action === 'image_edited' || rv.action === 'file_replaced'; }).length;
    var cntDeleted = allRevs.filter(function (rv) { return rv.action === 'deleted' || rv.action === 'file_deleted'; }).length;

    body.innerHTML =
      '<div class="rf-section-head"><div><h2>🛡️ Audit Management, Anomaly Inspection &amp; Change Log</h2>' +
      '<p>Tracks created, added, updated, and deleted items with Before/After diffs, plus Rules Engine anomaly detection, statistical outliers, and formal DOCX/PDF audit reports.</p></div>' +
      '<div style="display:flex;gap:8px;flex-wrap:wrap">' +
      '<button type="button" class="rf-btn rf-btn-primary" id="rf-aud-explain-btn">✨ AI Explain Anomalies</button>' +
      '<button type="button" class="rf-btn" id="rf-aud-sql-btn">🗄️ Inspect SQL Logs</button>' +
      '<button type="button" class="rf-btn" id="rf-aud-disk-btn">🔍 Scan Disk Changes</button>' +
      '<a class="rf-btn" href="' + esc(apiUrl('/admin/api/audit/report/docx')) + '" download="arkib_audit_report.docx">📄 DOCX Report</a>' +
      '<a class="rf-btn" href="' + esc(apiUrl('/admin/api/audit/report/pdf')) + '" download="arkib_audit_report.pdf">📕 PDF Report</a>' +
      '</div></div>' +
      '<div class="rf-kpi-grid">' +
      '<div class="rf-kpi-card"><div class="rf-kpi-lbl">Overall Risk</div><div class="rf-kpi-val"><span class="rf-badge ' + sevCls(snap.risk_level) + '" style="font-size:.95rem;padding:4px 10px">' + esc(snap.risk_level) + '</span> <span style="font-size:.9rem">' + Number(snap.risk_score || 0) + '/100</span></div></div>' +
      '<div class="rf-kpi-card"><div class="rf-kpi-lbl">Events Analyzed</div><div class="rf-kpi-val">' + Number(snap.total_events_analyzed || 0) + '</div></div>' +
      '<div class="rf-kpi-card"><div class="rf-kpi-lbl">Rules Engine Alerts</div><div class="rf-kpi-val">' + alertCount + '</div></div>' +
      '<div class="rf-kpi-card"><div class="rf-kpi-lbl">Statistical Outliers</div><div class="rf-kpi-val">' + outlierCount + '</div></div>' +
      '<div class="rf-kpi-card"><div class="rf-kpi-lbl">Tracked Changes</div><div class="rf-kpi-val">' + allRevs.length + '</div></div>' +
      '</div>' +
      '<div class="rf-callout ' + calloutTone + '">' +
      '<div class="rf-callout-title" style="display:flex;align-items:center;justify-content:space-between;gap:10px;flex-wrap:wrap">' +
      '<div style="display:flex;align-items:center;gap:8px;flex-wrap:wrap"><span>' + statusHeadline + '</span>' +
      (sourceLabel ? '<span class="rf-badge rf-badge-info">' + esc(sourceLabel) + '</span>' : '') + '</div>' +
      (state.auditAiExplanation ? '<button type="button" class="rf-btn rf-btn-sm" id="rf-aud-clear-exp">✕ Dismiss</button>' : '') +
      '</div>' +
      summaryBodyHtml +
      '</div>' +
      diskHtml +
      '<div class="rf-audit-panel">' +
      '<div style="display:flex;justify-content:space-between;align-items:center;gap:10px;flex-wrap:wrap;margin-bottom:10px">' +
      '<h3 style="margin:0">🕒 Data &amp; File Changes (Created · Added · Updated · Deleted)</h3>' +
      '<div style="display:flex;gap:6px;flex-wrap:wrap">' +
      '<button type="button" class="rf-btn rf-btn-sm' + (state.auditChangeFilter === 'all' ? ' rf-btn-primary' : '') + '" data-rf-ch-flt="all">All (' + allRevs.length + ')</button>' +
      '<button type="button" class="rf-btn rf-btn-sm' + (state.auditChangeFilter === 'created' ? ' rf-btn-primary' : '') + '" data-rf-ch-flt="created">＋ Created / Added (' + cntCreated + ')</button>' +
      '<button type="button" class="rf-btn rf-btn-sm' + (state.auditChangeFilter === 'updated' ? ' rf-btn-primary' : '') + '" data-rf-ch-flt="updated">✎ Updated (' + cntUpdated + ')</button>' +
      '<button type="button" class="rf-btn rf-btn-sm' + (state.auditChangeFilter === 'deleted' ? ' rf-btn-primary' : '') + '" data-rf-ch-flt="deleted">✕ Deleted (' + cntDeleted + ')</button>' +
      '</div></div>' +
      (filteredRevs.length ? filteredRevs.map(function (rv) {
        var loc = resolveRevLocation(rv);
        var locBadge = '<span class="rf-badge ' + (loc.exists ? 'rf-badge-ok' : 'rf-badge-crit') + '">' + esc(loc.label) + '</span>';
        return '<div class="rf-alert-row" style="display:flex;justify-content:space-between;align-items:center;gap:10px;flex-wrap:wrap">' +
          '<div style="flex:1;min-width:240px">' +
          '<div style="display:flex;gap:6px;align-items:center;flex-wrap:wrap">' + changeActionBadge(rv.action) + '<span class="rf-badge rf-badge-info">' + esc(rv.entity_type) + '</span><strong>' + esc(rv.title || rv.slug) + '</strong>' + locBadge + '</div>' +
          '<div style="font-size:.83rem;margin-top:5px">' + esc(rv.summary) + '</div>' +
          '<div style="font-size:.75rem;color:rgb(var(--text-secondary));margin-top:3px">By <b>' + esc(rv.actor) + '</b> · ' + esc(fmtTime(rv.timestamp)) + '</div></div>' +
          '<div style="display:flex;gap:6px;flex-wrap:wrap">' +
          (loc.exists ? '<button type="button" class="rf-btn rf-btn-primary rf-btn-sm" data-rf-aud-goto="' + esc(rv.entity_type + '::' + rv.slug) + '">↗ Open Item</button>' : '') +
          '<button type="button" class="rf-btn rf-btn-sm" data-rf-aud-cmp="' + esc(rv.entity_type + '::' + rv.slug + '::' + (rv.title || rv.slug)) + '">🔍 Inspect / Compare</button>' +
          '</div></div>';
      }).join('') : '<div style="font-size:.84rem;color:rgb(var(--text-secondary))">No changes in this filter yet. Creating, adding, editing, or deleting any document, person, location, or category will appear here immediately.</div>') +
      '</div>' +
      '<div class="rf-audit-panel"><h3>🚨 Rules Engine Alerts (' + alertCount + ')</h3>' +
      (alertCount ? snap.rule_alerts.map(function (a) {
        return '<div class="rf-alert-row">' +
          '<div style="display:flex;gap:8px;align-items:center;flex-wrap:wrap"><span class="rf-badge ' + sevCls(a.severity) + '">' + esc(a.severity) + '</span><span class="rf-badge">' + esc(a.rule_id) + '</span><strong>' + esc(a.title) + '</strong><span style="margin-left:auto;font-size:.76rem;color:rgb(var(--text-secondary))">' + esc(a.actor) + ' · ' + esc(fmtTime(a.timestamp)) + '</span></div>' +
          '<div style="font-size:.83rem;margin-top:4px">' + esc(a.description) + '</div>' +
          '<div style="font-size:.78rem;margin-top:4px;color:rgb(var(--text-secondary))"><b>Recommended Action:</b> ' + esc(a.recommended_action) + '</div>' +
          '</div>';
      }).join('') : '<div style="font-size:.84rem;color:#10b981">✓ No suspicious CRUD bursts, login brute-force attempts, or SQL/NoSQL injection probes detected.</div>') +
      '</div>' +
      '<div class="rf-audit-panel"><h3>📈 Statistical Outlier Findings (' + outlierCount + ')</h3>' +
      (outlierCount ? snap.statistical_outliers.map(function (o) {
        return '<div class="rf-alert-row"><span class="rf-badge ' + sevCls(o.severity) + '">' + esc(o.severity) + '</span> <b>' + esc(o.metric) + '</b> (Z=' + o.z_score + ') — ' + esc(o.explanation) + '</div>';
      }).join('') : '<div style="font-size:.84rem;color:rgb(var(--text-secondary))">No behavioral outliers detected against current rolling baselines.</div>') +
      '</div>' +
      '<div class="rf-audit-panel"><h3>📜 Recent Access &amp; CRUD Activity (' + (snap.recent_events ? snap.recent_events.length : 0) + ')</h3>' +
      '<div style="overflow-x:auto"><table class="rf-audit-table"><thead><tr><th>When</th><th>Actor</th><th>Action</th><th>Activity Summary</th><th>Status</th><th>Speed</th></tr></thead><tbody>' +
      (snap.recent_events && snap.recent_events.length ? snap.recent_events.slice(0, 25).map(function (ev) {
        var actCls = ev.action === 'DELETE' || ev.action === 'LOGIN_FAIL' ? 'rf-badge-crit'
          : ev.action === 'CREATE' || ev.action === 'LOGIN_OK' ? 'rf-badge-ok'
          : ev.action === 'UPDATE' ? 'rf-badge-warn' : '';
        return '<tr><td style="white-space:nowrap">' + esc(fmtTime(ev.timestamp)) + '</td><td><b>' + esc(ev.actor) + '</b></td><td><span class="rf-badge ' + actCls + '">' + esc(ev.action) + '</span></td><td>' + esc(humanAuditDescription(ev)) + '</td><td>' + Number(ev.status_code || 0) + '</td><td>' + Number(ev.duration_ms || 0) + ' ms</td></tr>';
      }).join('') : '<tr><td colspan="6" style="text-align:center;color:rgb(var(--text-secondary))">No audit events recorded yet.</td></tr>') +
      '</tbody></table></div></div>';

    var clrExpBtn = document.getElementById('rf-aud-clear-exp');
    if (clrExpBtn) {
      clrExpBtn.onclick = function () {
        state.auditAiExplanation = '';
        state.auditAiSource = '';
        renderAudit(body);
      };
    }
    body.querySelectorAll('[data-rf-ch-flt]').forEach(function (b) {
      b.onclick = function () {
        state.auditChangeFilter = b.getAttribute('data-rf-ch-flt');
        renderAudit(body);
      };
    });
    document.getElementById('rf-aud-explain-btn').onclick = async function (ev) {
      var btn = ev.target;
      btn.disabled = true;
      btn.textContent = '⏳ Analyzing with AI…';
      var r = await api('/admin/api/audit/explain', { method: 'POST' });
      btn.disabled = false;
      btn.textContent = '✨ AI Explain Anomalies';
      if (r.ok && r.data) {
        state.auditAiExplanation = r.data.explanation || '';
        state.auditAiSource = r.data.source || 'ai';
        toast('Audit explanation updated');
        renderAudit(body);
      } else toast('Failed explaining anomalies', false);
    };
    document.getElementById('rf-aud-sql-btn').onclick = function () {
      openSqlInspectModal(function () { refreshAll(false); });
    };
    document.getElementById('rf-aud-disk-btn').onclick = async function (ev) {
      var btn = ev.target;
      btn.disabled = true;
      btn.textContent = '⏳ Scanning…';
      var r = await api('/admin/api/revisions/scan-disk', { method: 'POST' });
      btn.disabled = false;
      btn.textContent = '🔍 Scan Disk Changes';
      if (r.ok && r.data) {
        state.diskScanResult = r.data;
        toast('Disk scan complete (' + Number(r.data.modified_count || 0) + ' modified)');
        renderAudit(body);
      } else toast('Disk scan failed', false);
    };
    body.querySelectorAll('[data-rf-aud-goto]').forEach(function (b) {
      b.onclick = function () {
        var parts = b.getAttribute('data-rf-aud-goto').split('::');
        var et = parts[0], sl = parts[1];
        if (et === 'document') {
          var d = state.documents.find(function (x) { return x.slug === sl; });
          state.tab = 'documents';
          state.selectedDocGroup = 'all';
          state.docCategoryFilter = 'all';
          state.docSourceFilter = 'all';
          state.search = '';
          render();
          if (d) openDocumentModal(d);
        } else if (et === 'profile') {
          var p = state.profiles.find(function (x) { return x.slug === sl; });
          state.tab = 'people';
          state.selectedGroup = 'all';
          state.search = '';
          render();
          if (p) openPersonModal(p);
        } else if (et === 'location') {
          var l = state.locations.find(function (x) { return x.slug === sl; });
          state.tab = 'locations';
          state.selectedLocGroup = 'all';
          state.search = '';
          render();
          if (l) openLocationModal(l);
        } else if (et === 'category') {
          state.tab = 'documents';
          render();
          openCategoriesModal();
        }
      };
    });
    body.querySelectorAll('[data-rf-aud-cmp]').forEach(function (b) {
      b.onclick = function () {
        var parts = b.getAttribute('data-rf-aud-cmp').split('::');
        openRevisionsModal(parts[0], parts[1], parts[2] || parts[1]);
      };
    });
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
      '<p style="color:rgb(var(--text-secondary));margin-bottom:10px">Upload a photo or manuscript scan to preview it immediately, inspect quality/damage/authenticity, or enhance clarity with side-by-side Before vs. After comparison.</p>' +
      '<div class="rf-toolbar" style="margin-bottom:10px">' +
      '<input class="rf-input" id="rf-lab-file" type="file" accept=".jpg,.jpeg,.png,.webp,.bmp"/>' +
      '<select class="rf-select" id="rf-lab-mode">' +
      '<option value="ai_upscale_4x"' + (state.labMode === 'ai_upscale_4x' ? ' selected' : '') + '>🚀 AI Upscale 4× (Real-ESRGAN Neural SR)</option>' +
      '<option value="ai_upscale_2x"' + (state.labMode === 'ai_upscale_2x' ? ' selected' : '') + '>🚀 AI Upscale 2× (Real-ESRGAN Fast SR)</option>' +
      '<option value="ai_upscale_anime"' + (state.labMode === 'ai_upscale_anime' ? ' selected' : '') + '>🎨 AI Upscale 4× (Illustrations / Logos / Graphics)</option>' +
      '<option value="auto"' + (state.labMode === 'auto' ? ' selected' : '') + '>Mode: Auto Archival Restore</option>' +
      '<option value="manuscript_ocr"' + (state.labMode === 'manuscript_ocr' ? ' selected' : '') + '>Mode: Manuscript OCR Boost</option>' +
      '<option value="deyellow_restore"' + (state.labMode === 'deyellow_restore' ? ' selected' : '') + '>Mode: De-Yellow &amp; Paper Restore</option>' +
      '<option value="contrast_lighting"' + (state.labMode === 'contrast_lighting' ? ' selected' : '') + '>Mode: Lighting &amp; Contrast Fix</option>' +
      '<option value="denoise_sharpen"' + (state.labMode === 'denoise_sharpen' ? ' selected' : '') + '>Mode: Crisp Sharpen</option>' +
      '</select>' +
      '<button type="button" class="rf-btn rf-btn-primary" id="rf-lab-ana">🔬 Analyze Quality &amp; Damage</button>' +
      '<button type="button" class="rf-btn rf-btn-ok" id="rf-lab-enh">✨ Enhance Image</button>' +
      (state.labFile ? '<button type="button" class="rf-btn rf-btn-danger rf-btn-sm" id="rf-lab-clr">' + TRASH_SVG + '<span>Clear</span></button>' : '') +
      '</div>' +
      '<div id="rf-lab-stage"></div>' +
      '<div id="rf-lab-out">' + state.labHtml + '</div></div>';

    function renderLabStage() {
      var stageEl = document.getElementById('rf-lab-stage');
      if (!stageEl) return;
      if (!state.labPreviewUrl) {
        stageEl.innerHTML = '';
        return;
      }
      var sizeKb = state.labFile && state.labFile.size ? ' (' + Math.max(1, Math.round(state.labFile.size / 1024)) + ' KB)' : '';
      if (state.labEnhancedUrl) {
        var meta = state.labEnhancedMeta || {};
        var bq = meta.before_quality || {};
        var aq = meta.after_quality || {};
        stageEl.innerHTML =
          '<div class="rf-lab-compare-wrap">' +
          '<div class="rf-lab-compare-head">' +
          '<div style="display:flex;align-items:center;gap:8px;flex-wrap:wrap">' +
          '<span class="rf-badge rf-badge-ok">✨ Enhanced (' + esc(meta.mode || state.labMode || 'auto') + ')</span>' +
          (meta.upscale_engine ? '<span class="rf-badge rf-badge-info">⚡ ' + esc(meta.upscale_engine) + '</span>' : '') +
          (meta.before_dimensions && meta.after_dimensions
            ? '<span class="rf-badge rf-badge-ok">📐 ' + esc(meta.before_dimensions) + ' → ' + esc(meta.after_dimensions) + '</span>'
            : '') +
          (bq.overall_score != null && aq.overall_score != null
            ? '<span class="rf-badge rf-badge-info">Quality: ' + esc(bq.overall_score) + ' → ' + esc(aq.overall_score) + '/100</span>'
            : '') +
          (bq.readability && aq.readability
            ? '<span class="rf-badge rf-badge-info">Readability: ' + esc(bq.readability.score) + ' → ' + esc(aq.readability.score) + '</span>'
            : '') +
          '</div>' +
          '<a class="rf-btn rf-btn-sm" href="' + esc(state.labEnhancedUrl) + '" download="enhanced_' + esc(state.labFileName || 'image.png') + '">⬇️ Download Enhanced</a>' +
          '</div>' +
          '<div class="rf-lab-compare-grid">' +
          '<div class="rf-lab-img-card">' +
          '<div class="rf-lab-img-head"><span class="rf-badge rf-badge-warn">BEFORE · ORIGINAL</span><span class="rf-lab-img-name">' + esc(state.labFileName) + (meta.before_dimensions ? ' · ' + esc(meta.before_dimensions) : '') + esc(sizeKb) + '</span></div>' +
          '<div class="rf-lab-img-frame"><img src="' + esc(state.labPreviewUrl) + '" id="rf-lab-before-img" alt="Original uploaded image"/></div>' +
          '</div>' +
          '<div class="rf-lab-img-card rf-lab-img-card-after">' +
          '<div class="rf-lab-img-head"><span class="rf-badge rf-badge-ok">AFTER · ENHANCED</span><span class="rf-lab-img-name">' + esc(meta.mode || state.labMode || 'auto') + (meta.after_dimensions ? ' · ' + esc(meta.after_dimensions) : '') + '</span></div>' +
          '<div class="rf-lab-img-frame"><img src="' + esc(state.labEnhancedUrl) + '" id="rf-lab-after-img" alt="Enhanced image"/></div>' +
          '</div>' +
          '</div></div>';
        var bImg = document.getElementById('rf-lab-before-img');
        if (bImg) bImg.onclick = function () { openViewer(state.labPreviewUrl); };
        var aImg = document.getElementById('rf-lab-after-img');
        if (aImg) aImg.onclick = function () { openViewer(state.labEnhancedUrl); };
      } else {
        stageEl.innerHTML =
          '<div class="rf-lab-single-wrap">' +
          '<div class="rf-lab-img-card">' +
          '<div class="rf-lab-img-head"><span class="rf-badge rf-badge-info">🖼️ UPLOADED ORIGINAL</span><span class="rf-lab-img-name">' + esc(state.labFileName) + esc(sizeKb) + '</span></div>' +
          '<div class="rf-lab-img-frame"><img src="' + esc(state.labPreviewUrl) + '" id="rf-lab-orig-img" alt="Uploaded image preview"/></div>' +
          '</div></div>';
        var oImg = document.getElementById('rf-lab-orig-img');
        if (oImg) oImg.onclick = function () { openViewer(state.labPreviewUrl); };
      }
    }
    renderLabStage();

    document.getElementById('rf-lab-file').onchange = function (e) {
      var f = e.target.files && e.target.files[0];
      if (!f) return;
      if (state.labPreviewUrl) {
        try { URL.revokeObjectURL(state.labPreviewUrl); } catch (_) {}
      }
      state.labFile = f;
      state.labFileName = f.name || 'uploaded-image.jpg';
      state.labPreviewUrl = URL.createObjectURL(f);
      state.labEnhancedUrl = '';
      state.labEnhancedMeta = null;
      state.labHtml = '';
      renderSystem(body);
    };
    document.getElementById('rf-lab-mode').onchange = function (e) {
      state.labMode = e.target.value;
    };
    var labClr = document.getElementById('rf-lab-clr');
    if (labClr) {
      labClr.onclick = function () {
        if (state.labPreviewUrl) {
          try { URL.revokeObjectURL(state.labPreviewUrl); } catch (_) {}
        }
        state.labFile = null;
        state.labFileName = '';
        state.labPreviewUrl = '';
        state.labEnhancedUrl = '';
        state.labEnhancedMeta = null;
        state.labHtml = '';
        renderSystem(body);
      };
    }

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
      var f = state.labFile || (document.getElementById('rf-lab-file').files && document.getElementById('rf-lab-file').files[0]);
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
        '<div style="display:grid;gap:8px;margin-top:12px">' +
        '<div><b>📐 Image Quality:</b> <b>' + (q.overall_score || 0) + '/100 (' + esc(q.overall_grade) + ')</b> · Resolution: ' + (q.resolution ? q.resolution.width + '×' + q.resolution.height : '') + ' · Lighting: ' + (q.lighting ? q.lighting.score + '/100 (' + esc(q.lighting.condition) + ')' : '') + ' · Readability: ' + (q.readability ? q.readability.score + '/100' : '') + '</div>' +
        '<div><b>🩹 Damage (Overall ' + dmgBadge(dm.overall_severity) + '):</b> Tear ' + dmgBadge(dm.tear && dm.tear.level) + ' · Faded ' + dmgBadge(dm.faded && dm.faded.level) + ' · Mold ' + dmgBadge(dm.mold && dm.mold.level) + ' · Aging ' + dmgBadge(dm.aging && dm.aging.level) + '</div>' +
        authHtml +
        '<div><b>💊 Conservation &amp; Healing Recommendations:</b><ul>' + sugg + '</ul></div></div>';
      document.getElementById('rf-lab-out').innerHTML = state.labHtml;
    };
    document.getElementById('rf-lab-enh').onclick = async function () {
      var f = state.labFile || (document.getElementById('rf-lab-file').files && document.getElementById('rf-lab-file').files[0]);
      if (!f) { toast('Choose an image first', false); return; }
      var fd = new FormData();
      fd.append('image', f);
      state.labMode = document.getElementById('rf-lab-mode').value;
      fd.append('mode', state.labMode);
      var enhBtn = document.getElementById('rf-lab-enh');
      enhBtn.disabled = true;
      document.getElementById('rf-lab-out').innerHTML = '<div style="margin-top:10px">⏳ Enhancing image clarity…</div>';
      try {
        var r = await api('/admin/api/conservation/enhance', { method: 'POST', body: fd });
        if (!r.ok) {
          toast((r.data && r.data.reason) || 'Enhancement failed', false);
          document.getElementById('rf-lab-out').innerHTML = '';
          return;
        }
        var j = r.data;
        state.labEnhancedUrl = j.admin_preview_url ? apiUrl(j.admin_preview_url) + '?ts=' + Date.now() : '';
        state.labEnhancedMeta = j;
        state.labHtml = '';
        renderLabStage();
        document.getElementById('rf-lab-out').innerHTML = '';
        toast('Image enhanced — compare Before vs. After side-by-side');
      } finally {
        enhBtn.disabled = false;
      }
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
    var guestMode = isGuest();
    var sizeKb = state.testFile && state.testFile.size ? ' (' + Math.max(1, Math.round(state.testFile.size / 1024)) + ' KB)' : '';
    body.innerHTML =
      '<div class="rf-section-head"><div><h2>🔍 Test Face Recognition</h2><p>' +
      (guestMode
        ? 'Upload, drag &amp; drop, or paste a copied image (Ctrl+V) to test face matching and confidence scores against enrolled identities (read-only).'
        : 'Upload, drag &amp; drop, or paste a copied image (Ctrl+V) to test face matching and confidence scores. Unrecognized faces can be enrolled directly into the vector database.') +
      '</p></div></div>' +
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
    function updateEnrollHint() {
      if (guestMode) {
        enrollContainer.innerHTML = state.testUnrecognized
          ? '<div class="rf-guest-readonly-note">ℹ️ Unrecognized face detected. Enrolling new identities into the biometric database requires Administrator access.</div>'
          : '';
      } else {
        renderTestEnrollPrompt(enrollContainer, body);
      }
    }
    updateEnrollHint();

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
      updateEnrollHint();
      document.getElementById('rf-tf-out').innerHTML = '⏳ Running face detection and vector matching…';
      try {
        var endpoint = guestMode ? '/guest/api/test-recognition' : '/admin/api/test-recognition';
        var r = await api(endpoint, { method: 'POST', body: fd });
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
        updateEnrollHint();
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
      if (state.open && !isGuest()) closeAdmin();
      else openAdmin();
    });
    return btn;
  }

  function createGuestButton() {
    var btn = document.createElement('button');
    btn.type = 'button';
    btn.id = GUEST_BTN_ID;
    btn.setAttribute('data-testid', 'nav-panel-arkib-guest');
    btn.setAttribute('aria-label', 'Guest Portal (Read-Only)');
    btn.setAttribute('aria-pressed', state.open && isGuest() ? 'true' : 'false');
    btn.className = 'rf-admin-rail-btn rf-guest-rail-btn flex h-9 w-9 items-center justify-center rounded-lg transition-colors' + (state.open && isGuest() ? ' rf-active' : '');
    btn.innerHTML =
      '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="h-5 w-5" aria-hidden="true">' +
      '<path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0"/>' +
      '<circle cx="12" cy="12" r="3"/>' +
      '</svg>' +
      '<span class="rf-rail-tooltip">Guest Portal (Read-Only)</span>';
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
      if (state.open && isGuest()) closeAdmin();
      else openGuest();
    });
    return btn;
  }

  function mountAdminButton() {
    var mcpBtn = document.querySelector('[data-testid="nav-panel-mcp-builder"]');
    if (!mcpBtn || !mcpBtn.parentElement) return;
    state.railEl = mcpBtn.closest('.bg-surface-sidebar') || mcpBtn.parentElement.parentElement;
    var adminBtn = document.getElementById(BTN_ID);
    if (!adminBtn) {
      adminBtn = createAdminButton();
      mcpBtn.insertAdjacentElement('afterend', adminBtn);
    } else if (adminBtn.previousElementSibling !== mcpBtn) {
      mcpBtn.insertAdjacentElement('afterend', adminBtn);
    }
    var guestBtn = document.getElementById(GUEST_BTN_ID);
    if (!guestBtn) {
      guestBtn = createGuestButton();
      adminBtn.insertAdjacentElement('afterend', guestBtn);
    } else if (guestBtn.previousElementSibling !== adminBtn) {
      adminBtn.insertAdjacentElement('afterend', guestBtn);
    }
    if (state.open) positionStage();
  }

  document.addEventListener('click', function (ev) {
    if (!ev.target.closest || (!ev.target.closest('.rf-menu-pop') && !ev.target.closest('.rf-folder-act') && !ev.target.closest('.rf-card-menu-btn'))) {
      closeMenus();
    }
    if (!state.open) return;
    var target = ev.target && ev.target.closest ? ev.target.closest('button, a') : null;
    if (!target || target.id === BTN_ID || target.id === GUEST_BTN_ID) return;
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
    if (!state.open || (!state.authed && !isGuest()) || state.tab !== 'test') return;
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

  function handlePortalHash() {
    var h = window.location.hash || '';
    if (h === '#arkib-admin' || h === '#rojakface-admin') {
      setTimeout(openAdmin, 450);
    } else if (h === '#arkib-guest') {
      setTimeout(openGuest, 450);
    }
  }
  handlePortalHash();
  window.addEventListener('hashchange', handlePortalHash);
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
    lockedMode: false,
    sessionId: 'new',
    lastPathname: window.location.pathname,
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
    var mode = (parts[0] === 'enroll') ? 'enroll' : (parts[0] === 'compare') ? 'compare' : 'ocr';
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
      '    <span id="ac-header-title">🗂️ Arkib Canvas</span>' +
      '    <span class="ac-role-pill" id="ac-role-pill">Guest · Staged Approval</span>' +
      '  </div>' +
      '  <div style="display:flex;align-items:center;gap:6px">' +
      '    <button type="button" class="rf-btn" id="ac-clear-btn" style="padding:4px 9px;font-size:.72rem" title="Clear workspace">Clear</button>' +
      '    <button type="button" class="rf-btn" id="ac-close-btn" style="padding:4px 9px;font-size:.78rem" aria-label="Close Arkib Canvas">✕</button>' +
      '  </div>' +
      '</div>' +
      '<div class="ac-mode-tabs" id="ac-mode-tabs">' +
      '  <button type="button" class="ac-mode-tab" data-ac-mode="ocr">📜 Jawi OCR</button>' +
      '  <button type="button" class="ac-mode-tab" data-ac-mode="enroll">🪪 Enrollment</button>' +
      '  <button type="button" class="ac-mode-tab" data-ac-mode="compare">⚖️ Before / After</button>' +
      '</div>' +
      '<div class="ac-body" id="ac-body"><div class="rf-empty">Loading…</div></div>' +
      '<div class="ac-footer" id="ac-footer"></div>';
    document.body.appendChild(el);

    document.getElementById('ac-close-btn').onclick = closeCanvas;
    document.getElementById('ac-clear-btn').onclick = function () {
      loadSession(ac.mode, 'new');
    };
    el.querySelectorAll('[data-ac-mode]').forEach(function (btn) {
      btn.onclick = function () {
        var nextMode = btn.getAttribute('data-ac-mode');
        if (nextMode !== ac.mode) {
          openCanvas(nextMode, 'new', false);
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

  function resetCanvasState() {
    ac.sessionId = 'new';
    ac.data = null;
    ac.jawiText = '';
    ac.rumiText = '';
    ac.rumiManualOverride = false;
    ac.lexiconPairs = [];
    ac.enrollImg = null;
    ac.enrollImgSrc = '';
    ac.cropRect = null;
    ac.croppedDataUrl = '';
    ac.rotateDeg = 0;
    ac.brightness = 100;
    ac.contrast = 100;
  }

  function closeCanvas() {
    ac.open = false;
    ac.lockedMode = false;
    resetCanvasState();
    document.body.classList.remove('ac-canvas-open');
    var el = document.getElementById(DRAWER_ID);
    if (el) el.remove();
    updateRailButtonState();
    if (window.location.hash && window.location.hash.indexOf('#arkib-canvas:') === 0) {
      try { history.replaceState(null, '', window.location.pathname + window.location.search); } catch (_) {}
    }
  }

  async function openCanvas(mode, id, lockedMode) {
    ac.mode = (mode === 'enroll') ? 'enroll' : (mode === 'compare') ? 'compare' : 'ocr';
    ac.sessionId = id || 'new';
    ac.lockedMode = !!lockedMode;
    ac.lastPathname = window.location.pathname;
    ac.open = true;
    applyDrawerWidth(ac.widthVw + 'vw');
    document.body.classList.add('ac-canvas-open');
    var el = ensureDrawer();
    var tabsEl = document.getElementById('ac-mode-tabs');
    if (tabsEl) tabsEl.style.display = ac.lockedMode ? 'none' : 'flex';
    var titleEl = document.getElementById('ac-header-title');
    if (titleEl) {
      titleEl.textContent = ac.lockedMode
        ? (ac.mode === 'ocr' ? '📜 Jawi OCR Workbench' : ac.mode === 'compare' ? '⚖️ Before & After Comparison Studio' : '🪪 Identity Enrollment')
        : '🗂️ Arkib Canvas';
    }
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
    body.innerHTML = '<div class="rf-empty">Loading ' + (mode === 'ocr' ? 'Jawi OCR Workbench' : mode === 'compare' ? 'Before & After Comparison Studio' : 'Enrollment Canvas') + '…</div>';
    footer.innerHTML = '';

    try {
      var r = await acApi('/canvas/api/session/' + encodeURIComponent(mode) + '/' + encodeURIComponent(id || 'new'));
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
        ac.initialJawiText = ac.jawiText;
        ac.initialRumiText = ac.rumiText;
        ac.initialCoveragePct = typeof r.data.coverage_pct === 'number' ? r.data.coverage_pct : null;
        ac.latestCoveragePct = ac.initialCoveragePct;
        ac.latestHijriDates = Array.isArray(r.data.hijri_colophon_dates) ? r.data.hijri_colophon_dates : [];
        ac.ocrDiffMode = 'side';
        ac.rumiManualOverride = false;
        ac.lexiconPairs = [];
        renderOcrWorkbench(body, footer);
      } else if (mode === 'compare') {
        ac.compareView = 'slider';
        ac.compareDiffMode = 'side';
        ac.compareSliderPct = 50;
        ac.compareMetaChangedOnly = false;
        renderCompareCanvas(body, footer);
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
      '<div class="ac-card" id="ac-ocr-diff-card">' +
      '  <div class="ac-diff-toolbar">' +
      '    <div class="ac-seg-group">' +
      '      <button type="button" class="ac-seg-btn' + (ac.ocrDiffMode !== 'unified' ? ' active' : '') + '" data-ac-ocr-dm="side">Side-by-Side Diff</button>' +
      '      <button type="button" class="ac-seg-btn' + (ac.ocrDiffMode === 'unified' ? ' active' : '') + '" data-ac-ocr-dm="unified">Unified Diff</button>' +
      '    </div>' +
      '    <div class="ac-diff-pills" id="ac-ocr-diff-pills"></div>' +
      '  </div>' +
      '  <div id="ac-ocr-diff-view"></div>' +
      '</div>' +
      '<div class="ac-card">' +
      '  <div class="ac-card-head"><span>💾 Save to Archive (Optional)</span></div>' +
      '  <div style="font-size:.73rem;color:rgb(var(--text-secondary));line-height:1.45">Save your corrected Jawi &amp; Rumi text as an archival record under <b>Admin Portal → Documents</b>, or attach it to an existing document.</div>' +
      '  <div class="ac-form-grid">' +
      '    <div class="ac-field"><label>Save Under Document</label>' +
      '      <select id="ac-doc-slug" class="rf-input" style="width:100%">' +
      '        <option value="">Save as New Archival Document</option>' +
      docs.map(function (doc) {
        var sel = (d.document_slug && d.document_slug === doc.slug) ? ' selected' : '';
        return '<option value="' + esc(doc.slug) + '"' + sel + '>' + esc(doc.title || doc.slug) + '</option>';
      }).join('') +
      '      </select>' +
      '    </div>' +
      '    <div class="ac-field"><label>Document Title</label><input id="ac-ocr-title" class="rf-input" style="width:100%" placeholder="e.g. Surat Sultan 1324H" value="' + esc(d.file || '') + '"/></div>' +
      '  </div>' +
      '  <input id="ac-ocr-notes" class="rf-input" style="width:100%" placeholder="Notes (optional, e.g. page number or condition)…"/>' +
      '  <details style="margin-top:4px;border-top:1px solid rgb(var(--border-light));padding-top:8px">' +
      '    <summary style="cursor:pointer;font-size:.75rem;font-weight:600;color:rgb(var(--text-secondary))">📖 Teach a New Jawi Word to Dictionary (Optional)</summary>' +
      '    <div style="font-size:.72rem;color:rgb(var(--text-secondary));margin:6px 0">If any Jawi word was misread, add its correct Malay (Rumi) spelling here so ArkibVision translates it accurately next time.</div>' +
      '    <div style="display:flex;gap:8px;align-items:flex-start;flex-wrap:wrap">' +
      '      <div class="ac-field arkib-field" style="flex:1;min-width:150px"><label class="arkib-label" for="ac-lex-jawi">Jawi Word</label><input id="ac-lex-jawi" class="rf-input arkib-input" dir="rtl" placeholder="ڤرلمباݢأن"/></div>' +
      '      <div class="ac-field arkib-field" style="flex:1;min-width:150px"><label class="arkib-label" for="ac-lex-rumi">DBP Rumi Word</label><input id="ac-lex-rumi" class="rf-input arkib-input" placeholder="Perlembagaan"/></div>' +
      '      <button type="button" class="rf-btn" id="ac-lex-add" style="margin-top:24px">+ Add Word</button>' +
      '    </div>' +
      '    <div id="ac-lex-list" style="display:flex;flex-wrap:wrap;gap:6px;font-size:.74rem;margin-top:6px"></div>' +
      '  </details>' +
      '</div>';

    footer.innerHTML =
      '<div style="display:flex;align-items:center;gap:6px;flex-wrap:wrap">' +
      '  <button type="button" class="rf-btn" id="ac-dl-docx" style="padding:5px 10px;font-size:.75rem" title="Download Jawi + Rumi as Word (.docx)">📄 DOCX</button>' +
      '  <button type="button" class="rf-btn" id="ac-dl-pdf" style="padding:5px 10px;font-size:.75rem" title="Download Rumi + Metadata as PDF (.pdf)">📕 PDF</button>' +
      '</div>' +
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

    function computeWordDiffJs(beforeStr, afterStr) {
      var a = String(beforeStr || '').trim().split(/\s+/).filter(Boolean);
      var b = String(afterStr || '').trim().split(/\s+/).filter(Boolean);
      var n = a.length, m = b.length;
      if (n > 300 || m > 300) {
        var same = String(beforeStr || '').trim() === String(afterStr || '').trim();
        var bt = [{ text: String(beforeStr || '').trim(), kind: same ? 'eq' : 'del' }];
        var at = [{ text: String(afterStr || '').trim(), kind: same ? 'eq' : 'add' }];
        return { before: bt, after: at, unified: same ? bt : [bt[0], at[0]], changes: same ? 0 : 1 };
      }
      var dp = new Array(n + 1);
      for (var i = 0; i <= n; i++) dp[i] = new Uint16Array(m + 1);
      for (var ii = n - 1; ii >= 0; ii--) {
        for (var jj = m - 1; jj >= 0; jj--) {
          dp[ii][jj] = a[ii] === b[jj] ? dp[ii + 1][jj + 1] + 1 : Math.max(dp[ii + 1][jj], dp[ii][jj + 1]);
        }
      }
      var before = [], after = [], unified = [], changes = 0, x = 0, y = 0;
      while (x < n && y < m) {
        if (a[x] === b[y]) {
          var t = { text: a[x], kind: 'eq' };
          before.push(t); after.push(t); unified.push(t); x++; y++;
        } else if (dp[x + 1][y] >= dp[x][y + 1]) {
          var td = { text: a[x], kind: 'del' };
          before.push(td); unified.push(td); changes++; x++;
        } else {
          var ta = { text: b[y], kind: 'add' };
          after.push(ta); unified.push(ta); changes++; y++;
        }
      }
      while (x < n) { var td2 = { text: a[x++], kind: 'del' }; before.push(td2); unified.push(td2); changes++; }
      while (y < m) { var ta2 = { text: b[y++], kind: 'add' }; after.push(ta2); unified.push(ta2); changes++; }
      return { before: before, after: after, unified: unified, changes: changes };
    }

    function tokensToHtml(tokens) {
      if (!Array.isArray(tokens) || !tokens.length) return '<span style="opacity:.55">(empty)</span>';
      return tokens.map(function (t) {
        if (t.kind === 'del') return '<span class="ac-tok-del">' + esc(t.text) + '</span>';
        if (t.kind === 'add') return '<span class="ac-tok-add">' + esc(t.text) + '</span>';
        return esc(t.text);
      }).join(' ');
    }

    function renderOcrLiveDiff() {
      var viewEl = document.getElementById('ac-ocr-diff-view');
      var pillsEl = document.getElementById('ac-ocr-diff-pills');
      if (!viewEl) return;
      var pillsHtml = '';
      if (typeof ac.initialCoveragePct === 'number' && typeof ac.latestCoveragePct === 'number') {
        pillsHtml += '<span class="ac-st-pill ac-pill-ok">DBP Coverage: ' + ac.initialCoveragePct.toFixed(1) + '% → ' + ac.latestCoveragePct.toFixed(1) + '%</span>';
      } else if (typeof ac.latestCoveragePct === 'number') {
        pillsHtml += '<span class="ac-st-pill ac-pill-ok">DBP Coverage: ' + ac.latestCoveragePct.toFixed(1) + '%</span>';
      }
      if (Array.isArray(ac.latestHijriDates) && ac.latestHijriDates.length) {
        var h0 = ac.latestHijriDates[0];
        pillsHtml += '<span class="ac-st-pill ac-pill-gold">' + esc(h0.hijri_date) + ' → ' + esc(h0.gregorian_date_masihi) + '</span>';
      }
      if (pillsEl) pillsEl.innerHTML = pillsHtml;

      var jDiff = computeWordDiffJs(ac.initialJawiText || '', ac.jawiText || '');
      var rDiff = computeWordDiffJs(ac.initialRumiText || '', ac.rumiText || '');
      if (ac.ocrDiffMode === 'unified') {
        viewEl.innerHTML =
          '<div class="ac-diff-pane" style="margin-bottom:10px">' +
          '<div class="ac-diff-pane-lbl">UNIFIED DIFF · JAWI (RTL)</div>' +
          '<div class="ac-diff-pane-body ac-diff-rtl">' + tokensToHtml(jDiff.unified) + '</div></div>' +
          '<div class="ac-diff-pane">' +
          '<div class="ac-diff-pane-lbl">UNIFIED DIFF · STANDARD BAHASA MALAYSIA (RUMI)</div>' +
          '<div class="ac-diff-pane-body">' + tokensToHtml(rDiff.unified) + '</div></div>';
      } else {
        viewEl.innerHTML =
          '<div class="ac-diff-pair" style="margin-bottom:10px">' +
          '<div class="ac-diff-pane"><div class="ac-diff-pane-lbl">BEFORE · RAW OCR (JAWI RTL)</div><div class="ac-diff-pane-body ac-diff-rtl">' + tokensToHtml(jDiff.before) + '</div></div>' +
          '<div class="ac-diff-pane"><div class="ac-diff-pane-lbl">AFTER · CORRECTED (JAWI RTL)</div><div class="ac-diff-pane-body ac-diff-rtl">' + tokensToHtml(jDiff.after) + '</div></div>' +
          '</div>' +
          '<div class="ac-diff-pair">' +
          '<div class="ac-diff-pane"><div class="ac-diff-pane-lbl">BEFORE · RUMI OUTPUT (DBP)</div><div class="ac-diff-pane-body">' + tokensToHtml(rDiff.before) + '</div></div>' +
          '<div class="ac-diff-pane"><div class="ac-diff-pane-lbl">AFTER · VERIFIED STANDARD BM (RUMI)</div><div class="ac-diff-pane-body">' + tokensToHtml(rDiff.after) + '</div></div>' +
          '</div>';
      }
    }

    body.querySelectorAll('[data-ac-ocr-dm]').forEach(function (btn) {
      btn.onclick = function () {
        ac.ocrDiffMode = btn.getAttribute('data-ac-ocr-dm');
        body.querySelectorAll('[data-ac-ocr-dm]').forEach(function (b) {
          b.classList.toggle('active', b.getAttribute('data-ac-ocr-dm') === ac.ocrDiffMode);
        });
        renderOcrLiveDiff();
      };
    });
    renderOcrLiveDiff();

    // Jawi editor + 6-key helper bar + debounced 250ms deterministic preview
    var jawiTa = document.getElementById('ac-jawi-ta');
    var rumiTa = document.getElementById('ac-rumi-ta');
    var runPreview = async function () {
      var text = jawiTa.value;
      ac.jawiText = text;
      if (!text.trim()) {
        if (!ac.rumiManualOverride) { rumiTa.value = ''; ac.rumiText = ''; }
        renderOcrLiveDiff();
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
          if (typeof r.data.coverage_pct === 'number') {
            ac.latestCoveragePct = r.data.coverage_pct;
            if (ac.initialCoveragePct === null) ac.initialCoveragePct = r.data.coverage_pct;
            if (covEl) covEl.textContent = 'FST Coverage: ' + r.data.coverage_pct.toFixed(1) + '%';
          }
          var stEl = document.getElementById('ac-style-lbl');
          if (stEl) stEl.textContent = r.data.script_style === 'old_classical_jawi_or_mixed' ? 'Old Classical Jawi' : 'Modern Jawi';
          var hb = document.getElementById('ac-hijri-bar');
          var hd = Array.isArray(r.data.hijri_colophon_dates) ? r.data.hijri_colophon_dates : [];
          ac.latestHijriDates = hd;
          if (hb) {
            if (hd.length) {
              hb.style.display = 'block';
              hb.style.cssText = 'font-size:.74rem;color:#fbbf24;padding:4px 8px;border-radius:6px;background:rgba(245,158,11,.1)';
              hb.innerHTML = '📅 Colophon Date: ' + hd.map(function (h) { return esc(h.hijri_date) + ' → ' + esc(h.gregorian_date_masihi); }).join(' · ');
            } else {
              hb.style.display = 'none';
            }
          }
          renderOcrLiveDiff();
        }
      } catch (_) {}
    };
    var schedulePreview = function () {
      ac.rumiManualOverride = false;
      renderOcrLiveDiff();
      clearTimeout(ac.previewTimer);
      ac.previewTimer = setTimeout(runPreview, 250);
    };
    jawiTa.addEventListener('input', schedulePreview);
    rumiTa.addEventListener('input', function () {
      ac.rumiText = rumiTa.value;
      ac.rumiManualOverride = true;
      renderOcrLiveDiff();
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
      var jEl = document.getElementById('ac-lex-jawi');
      var rEl = document.getElementById('ac-lex-rumi');
      var j = jEl.value.trim();
      var r = rEl.value.trim();
      if (window.ArkibForm) {
        window.ArkibForm.clearFieldError(jEl);
        window.ArkibForm.clearFieldError(rEl);
      }
      if (!j || !r) {
        if (window.ArkibForm) {
          if (!j) window.ArkibForm.setFieldError(jEl, 'Enter a Jawi word.');
          if (!r) window.ArkibForm.setFieldError(rEl, 'Enter the DBP Rumi spelling.');
        } else {
          acToast('Enter both Jawi and DBP Rumi words.', false);
        }
        (!j ? jEl : rEl).focus();
        return;
      }
      ac.lexiconPairs.push({ jawi: j, rumi: r });
      jEl.value = '';
      rEl.value = '';
      renderLexList();
    };
    renderLexList();

    // Instant DOCX / PDF export straight from Jawi OCR Workbench
    var downloadExport = async function (fmt) {
      var jawiVal = jawiTa.value.trim();
      var rumiVal = rumiTa.value.trim();
      if (!jawiVal && !rumiVal) { acToast('Enter Jawi or Rumi text before downloading.', false); return; }
      var titleVal = (document.getElementById('ac-ocr-title') && document.getElementById('ac-ocr-title').value.trim()) || 'Jawi Archival Transcription';
      var notesVal = (document.getElementById('ac-ocr-notes') && document.getElementById('ac-ocr-notes').value.trim()) || '';
      try {
        var resp = await fetch(API_ORIGIN + '/canvas/api/export-transcription/' + fmt, {
          method: 'POST',
          credentials: 'include',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ title: titleVal, jawi_text: jawiVal, rumi_text: rumiVal, notes: notesVal })
        });
        if (!resp.ok) { acToast('Export failed (' + resp.status + ')', false); return; }
        var blob = await resp.blob();
        var url = URL.createObjectURL(blob);
        var a = document.createElement('a');
        a.href = url;
        a.download = titleVal.toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_|_$/g, '') + '.' + fmt;
        document.body.appendChild(a);
        a.click();
        a.remove();
        setTimeout(function () { URL.revokeObjectURL(url); }, 1500);
        acToast('✅ Downloaded ' + fmt.toUpperCase() + ' file!', true);
      } catch (err) {
        acToast('Export error: ' + err.message, false);
      }
    };
    document.getElementById('ac-dl-docx').onclick = function () { downloadExport('docx'); };
    document.getElementById('ac-dl-pdf').onclick = function () { downloadExport('pdf'); };

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
      var existingEl = document.getElementById('ac-enr-existing');
      var newNameEl = document.getElementById('ac-enr-name');
      if (window.ArkibForm) {
        window.ArkibForm.clearFieldError(existingEl);
        window.ArkibForm.clearFieldError(newNameEl);
      }
      var existingId = isExist && existingEl ? existingEl.value.trim() : '';
      var newName = !isExist && newNameEl ? newNameEl.value.trim() : '';
      var target = existingId || newName;
      if (!target) {
        var targetEl = isExist ? existingEl : newNameEl;
        if (window.ArkibForm && targetEl) {
          window.ArkibForm.setFieldError(targetEl, isExist ? 'Please select an enrolled person.' : 'Person name is required.');
          targetEl.focus();
        } else {
          acToast('Please enter or select a person name.', false);
        }
        return;
      }
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
      else openCanvas(ac.mode || 'ocr', 'new', false);
    });
    return btn;
  }

  function mountCanvasButton() {
    var anchorBtn = document.getElementById('rf-guest-rail-btn') || document.getElementById('rf-admin-rail-btn');
    if (!anchorBtn || !anchorBtn.parentElement) return;
    var existing = document.getElementById(RAIL_BTN_ID);
    if (existing) {
      if (existing.previousElementSibling !== anchorBtn) anchorBtn.insertAdjacentElement('afterend', existing);
      return;
    }
    anchorBtn.insertAdjacentElement('afterend', createCanvasRailButton());
  }

  // Intercept any chat link matching `#arkib-canvas:<mode>:<id>` -> opens ONLY that specific workbench!
  document.addEventListener('click', function (ev) {
    var target = ev.target && ev.target.closest ? ev.target.closest('button, a') : null;
    if (target) {
      var tid = target.getAttribute('data-testid') || '';
      if (tid === 'new-chat-button' || tid === 'nav-new-chat-button') {
        if (ac.open) closeCanvas();
        return;
      }
    }
    var a = ev.target && ev.target.closest ? ev.target.closest('a[href*="#arkib-canvas:"]') : null;
    if (!a) return;
    var parsed = parseCanvasHash(a.getAttribute('href') || '');
    if (!parsed) return;
    ev.preventDefault();
    ev.stopPropagation();
    openCanvas(parsed.mode, parsed.id, true);
  }, true);

  window.addEventListener('hashchange', function () {
    var parsed = parseCanvasHash(window.location.hash || '');
    if (parsed) openCanvas(parsed.mode, parsed.id, true);
  });

  // Automatically close & wipe Canvas whenever user switches conversations or starts a New Chat (/c/new)
  setInterval(function () {
    if (window.location.pathname !== ac.lastPathname) {
      ac.lastPathname = window.location.pathname;
      if (ac.open) closeCanvas();
    }
  }, 450);

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

  // ─── Mode 3: Interactive Before & After Comparison Studio ─────────────────
  function renderCompareCanvas(body, footer) {
    var d = ac.data || {};
    var docs = Array.isArray(d.documents) ? d.documents : [];
    var hasImages = !!(d.before_image_url && d.after_image_url);
    var consRows = Array.isArray(d.conservation_metrics) ? d.conservation_metrics : [];
    var metaRows = Array.isArray(d.intake_metadata) ? d.intake_metadata : [];
    var pagesInfo = d.document_pages || null;
    var jawiPages = d.jawi_pages || null;
    var fidelity = d.migration_fidelity && typeof d.migration_fidelity === 'object' ? d.migration_fidelity : null;

    function statusPill(st) {
      var s = String(st || '');
      var lc = s.toLowerCase();
      var cls = (lc.indexOf('improv') !== -1 || lc === 'added' || lc === 'pass' || lc === 'preserved' || lc === 'high fidelity' || lc === 'verified new')
        ? 'ac-pill-ok'
        : (lc === 'removed' || lc === 'degraded' || lc === 'review' || lc.indexOf('stripped') !== -1 || lc.indexOf('major') !== -1)
          ? 'ac-pill-crit'
          : (lc === 'updated' || lc === 'changed' || lc === 'enhanced' || lc === 'resized' || lc.indexOf('minor') !== -1)
            ? 'ac-pill-warn'
            : 'ac-pill-muted';
      return '<span class="ac-st-pill ' + cls + '">' + esc(s || '—') + '</span>';
    }

    function renderTokenStream(tokens, side) {
      if (!Array.isArray(tokens) || !tokens.length) return '<span style="opacity:.6">(empty)</span>';
      return tokens.map(function (t) {
        if (t.kind === 'del') return '<span class="ac-tok-del">' + esc(t.text) + '</span>';
        if (t.kind === 'add') return '<span class="ac-tok-add">' + esc(t.text) + '</span>';
        return esc(t.text);
      }).join(' ');
    }

    function renderChangedPagesSection(pgObj, headingLabel, forceRtl) {
      if (!pgObj || !Number(pgObj.total_pages || 0)) return '';
      var changed = Array.isArray(pgObj.changed_pages) ? pgObj.changed_pages : [];
      var total = Number(pgObj.total_pages || 0);
      var unchangedCnt = Number(pgObj.unchanged_page_count || 0);
      var isUnified = ac.compareDiffMode === 'unified';
      var covPill = (typeof d.dbp_coverage_before === 'number' && typeof d.dbp_coverage_after === 'number')
        ? '<span class="ac-st-pill ac-pill-ok">DBP Coverage: ' + d.dbp_coverage_before.toFixed(1) + '% → ' + d.dbp_coverage_after.toFixed(1) + '%</span>'
        : '';
      var hijriPill = (d.hijri_date && d.gregorian_date_masihi)
        ? '<span class="ac-st-pill ac-pill-gold">' + esc(d.hijri_date) + ' → ' + esc(d.gregorian_date_masihi) + '</span>'
        : '';
      var badge = '<span class="ac-st-pill ac-pill-ok">Showing ' + changed.length + ' changed page(s) of ' + total + ' total' + (unchangedCnt > 0 ? ' · ' + unchangedCnt + ' unchanged hidden' : '') + '</span>';
      var toggleBar =
        '<div class="ac-diff-toolbar">' +
        '  <div class="ac-seg-group">' +
        '    <button type="button" class="ac-seg-btn' + (!isUnified ? ' active' : '') + '" data-ac-cdm="side">Side-by-Side Diff</button>' +
        '    <button type="button" class="ac-seg-btn' + (isUnified ? ' active' : '') + '" data-ac-cdm="unified">Unified Diff</button>' +
        '  </div>' +
        '  <div class="ac-diff-pills">' + covPill + hijriPill + badge + '</div>' +
        '</div>';

      if (!changed.length) {
        return '<div class="ac-cmp-card"><div class="ac-cmp-head"><strong>' + esc(headingLabel) + '</strong></div>' + toggleBar +
          '<div style="font-size:.82rem;color:#34d399">✓ Original and Latest pages are identical — no text differences found.</div></div>';
      }
      return '<div class="ac-cmp-card"><div class="ac-cmp-head"><strong>' + esc(headingLabel) + '</strong></div>' + toggleBar +
        changed.map(function (pg) {
          var rtlCls = (forceRtl || pg.is_rtl) ? ' ac-diff-rtl' : '';
          var uniTokens = Array.isArray(pg.unified_tokens) && pg.unified_tokens.length
            ? pg.unified_tokens
            : (pg.before_tokens || []).concat(pg.after_tokens || []);
          var contentHtml = isUnified
            ? '<div class="ac-diff-pane"><div class="ac-diff-pane-lbl">' + (forceRtl || pg.is_rtl ? 'UNIFIED DIFF · JAWI (RTL)' : 'UNIFIED DIFF · STANDARD BAHASA MALAYSIA (RUMI)') + ' · PAGE ' + Number(pg.page || 1) + '</div><div class="ac-diff-pane-body' + rtlCls + '">' + renderTokenStream(uniTokens, 'unified') + '</div></div>'
            : '<div class="ac-diff-pair">' +
              '<div class="ac-diff-pane"><div class="ac-diff-pane-lbl">' + (forceRtl || pg.is_rtl ? 'BEFORE · RAW OCR (JAWI RTL)' : 'BEFORE · ORIGINAL') + ' (PAGE ' + Number(pg.page || 1) + ')</div><div class="ac-diff-pane-body' + rtlCls + '">' + renderTokenStream(pg.before_tokens, 'before') + '</div></div>' +
              '<div class="ac-diff-pane"><div class="ac-diff-pane-lbl">' + (forceRtl || pg.is_rtl ? 'AFTER · CORRECTED (JAWI RTL)' : 'AFTER · LATEST') + ' (PAGE ' + Number(pg.page || 1) + ')</div><div class="ac-diff-pane-body' + rtlCls + '">' + renderTokenStream(pg.after_tokens, 'after') + '</div></div>' +
              '</div>';
          return '<div class="ac-page-diff-block">' +
            '<div class="ac-page-diff-bar"><span>📄 Page ' + Number(pg.page || 1) + '</span>' + statusPill(pg.status) + '<span style="font-size:.73rem;opacity:.75">' + Number(pg.word_changes || 0) + ' word change(s)</span></div>' +
            contentHtml + '</div>';
        }).join('') +
        '</div>';
    }

    // 1. Source Picker / Upload Bar
    var docOpts = '<option value="">— Select Archived Document (Original vs Latest) —</option>' +
      docs.map(function (doc) {
        var sel = d.document_slug && d.document_slug === doc.slug ? ' selected' : '';
        return '<option value="' + esc(doc.slug) + '"' + sel + '>' + esc(doc.title || doc.slug) + ' (' + Number(doc.revision_count || 0) + ' rev)</option>';
      }).join('');

    var pickerHtml =
      '<div class="ac-cmp-card">' +
      '<div class="ac-cmp-head"><strong>⚖️ Select Files or Document to Compare</strong>' + (d.title ? '<span class="ac-st-pill ac-pill-muted">' + esc(d.title) + '</span>' : '') + '</div>' +
      (docs.length ? '<div class="ac-cmp-row"><select class="rf-select" id="ac-cmp-doc-sel" style="flex:1">' + docOpts + '</select><button type="button" class="rf-btn rf-btn-primary rf-btn-sm" id="ac-cmp-doc-btn">Compare Original vs Latest</button></div>' : '') +
      '<div class="ac-cmp-upload-grid">' +
      '<label class="ac-cmp-drop"><span>Before (Original Image / File)</span><input type="file" id="ac-cmp-before-in" accept=".jpg,.jpeg,.png,.webp,.bmp,.pdf,.docx,.txt,.md,.csv,.html,.xml,.json"/></label>' +
      '<label class="ac-cmp-drop"><span>After (Edited / Latest Image / File)</span><input type="file" id="ac-cmp-after-in" accept=".jpg,.jpeg,.png,.webp,.bmp,.pdf,.docx,.txt,.md,.csv,.html,.xml,.json"/></label>' +
      '</div>' +
      '<div style="display:flex;justify-content:flex-end;margin-top:8px"><button type="button" class="rf-btn rf-btn-primary rf-btn-sm" id="ac-cmp-run-btn">⚖️ Compare Uploaded Files</button></div>' +
      '</div>';

    // 2. Interactive Image Comparator (Split Slider | Side-by-Side)
    var imageStageHtml = '';
    if (hasImages) {
      var bUrl = apiUrl(d.before_image_url);
      var aUrl = apiUrl(d.after_image_url);
      var stageInner = '';
      if (ac.compareView === 'side') {
        stageInner =
          '<div class="ac-diff-pair">' +
          '<div class="ac-diff-pane"><div class="ac-diff-pane-lbl">' + esc(d.before_label || 'BEFORE · ORIGINAL') + '</div><img class="ac-cmp-img" src="' + esc(bUrl) + '" alt="Before"/></div>' +
          '<div class="ac-diff-pane"><div class="ac-diff-pane-lbl">' + esc(d.after_label || 'AFTER · LATEST') + '</div><img class="ac-cmp-img" src="' + esc(aUrl) + '" alt="After"/></div>' +
          '</div>';
      } else {
        stageInner =
          '<div class="ac-curtain-stage" id="ac-curtain-stage">' +
          '<img class="ac-curtain-img" src="' + esc(aUrl) + '" alt="After"/>' +
          '<div class="ac-curtain-before" id="ac-curtain-before" style="clip-path:inset(0 ' + (100 - ac.compareSliderPct) + '% 0 0)">' +
          '<img class="ac-curtain-img" src="' + esc(bUrl) + '" alt="Before"/>' +
          '</div>' +
          '<div class="ac-curtain-line" id="ac-curtain-line" style="left:' + ac.compareSliderPct + '%"><span class="ac-curtain-knob">↔</span></div>' +
          '<span class="ac-corner-tag ac-tag-l">BEFORE</span><span class="ac-corner-tag ac-tag-r">AFTER</span>' +
          '</div>' +
          '<div class="ac-slider-bar"><span>Before</span><input type="range" id="ac-curtain-range" min="0" max="100" value="' + ac.compareSliderPct + '"/><span>After</span></div>';
      }

      imageStageHtml =
        '<div class="ac-cmp-card">' +
        '<div class="ac-cmp-head"><strong>🖼️ Interactive Image Comparison</strong>' +
        '<div class="ac-seg-group">' +
        '<button type="button" class="ac-seg-btn' + (ac.compareView !== 'side' ? ' active' : '') + '" data-ac-cv="slider">Split Slider</button>' +
        '<button type="button" class="ac-seg-btn' + (ac.compareView === 'side' ? ' active' : '') + '" data-ac-cv="side">Side-by-Side</button>' +
        '</div></div>' + stageInner + '</div>';
    }

    // 3. Conservation & Integrity Metrics (Before -> After)
    var consHtml = consRows.length ? (
      '<div class="ac-cmp-card"><div class="ac-cmp-head"><strong>🛡️ Conservation &amp; Integrity Metrics (Before → After)</strong></div>' +
      '<div style="overflow-x:auto"><table class="ac-cmp-table"><thead><tr><th>Metric</th><th>Before</th><th>After</th><th>Delta</th><th>Status</th></tr></thead><tbody>' +
      consRows.map(function (r) {
        return '<tr><td><b>' + esc(r.metric) + '</b></td><td>' + esc(r.before) + '</td><td>' + esc(r.after) + '</td><td>' + esc(r.delta) + '</td><td>' + statusPill(r.status) + '</td></tr>';
      }).join('') +
      '</tbody></table></div></div>'
    ) : '';

    // 4. Document Pages (Original vs Latest — Changed Pages Only)
    var pagesHtml = renderChangedPagesSection(jawiPages, '📜 Jawi Manuscript Changes (Original vs Latest · Changed Pages Only)', true) +
      renderChangedPagesSection(pagesInfo, '📄 Document Text Changes (Original vs Latest · Changed Pages Only)', false);

    // 5. Intake Metadata Comparison (Before -> After): field, before, after, status
    var visibleMeta = ac.compareMetaChangedOnly
      ? metaRows.filter(function (m) { return m.status !== 'Unchanged'; })
      : metaRows;
    var metaHtml = metaRows.length ? (
      '<div class="ac-cmp-card"><div class="ac-cmp-head"><strong>📋 Intake Metadata Comparison (Before → After)</strong>' +
      '<label style="font-size:.75rem;display:inline-flex;align-items:center;gap:5px;cursor:pointer"><input type="checkbox" id="ac-meta-chg-only"' + (ac.compareMetaChangedOnly ? ' checked' : '') + '/> Changed fields only</label></div>' +
      '<div style="overflow-x:auto"><table class="ac-cmp-table"><thead><tr><th>Field</th><th>Before</th><th>After</th><th>Status</th></tr></thead><tbody>' +
      (visibleMeta.length ? visibleMeta.map(function (m) {
        return '<tr><td><b>' + esc(m.label || m.field) + '</b></td><td class="' + (m.status === 'Updated' || m.status === 'Removed' ? 'ac-cell-before' : '') + '">' + esc(m.before) + '</td><td class="' + (m.status === 'Updated' || m.status === 'Added' ? 'ac-cell-after' : '') + '">' + esc(m.after) + '</td><td>' + statusPill(m.status) + '</td></tr>';
      }).join('') : '<tr><td colspan="4" style="text-align:center;opacity:.7">No metadata fields changed.</td></tr>') +
      '</tbody></table></div></div>'
    ) : '';

    // 6. Format Migration Fidelity
    var fidelityHtml = fidelity ? (
      '<div class="ac-cmp-card"><div class="ac-cmp-head"><strong>📦 Format Migration Fidelity</strong>' + statusPill(fidelity.verdict || 'PASS') + '</div>' +
      '<div class="ac-fid-grid">' +
      '<div class="ac-fid-cell"><span>Format</span><strong>' + esc(fidelity.source_format) + ' → ' + esc(fidelity.target_format) + '</strong></div>' +
      '<div class="ac-fid-cell"><span>File Size</span><strong>' + esc(fidelity.before_size_human) + ' → ' + esc(fidelity.after_size_human) + '</strong></div>' +
      '<div class="ac-fid-cell"><span>Dimensions</span><strong>' + esc(fidelity.before_dimensions) + ' → ' + esc(fidelity.after_dimensions) + ' ' + statusPill(fidelity.dimensions_status) + '</strong></div>' +
      '<div class="ac-fid-cell"><span>Visual Fidelity (aHash)</span><strong>' + (fidelity.visual_similarity_pct != null ? fidelity.visual_similarity_pct + '% · ' : '') + statusPill(fidelity.visual_status) + '</strong></div>' +
      '<div class="ac-fid-cell"><span>Metadata / EXIF Retention</span><strong>' + statusPill(fidelity.metadata_retention) + '</strong></div>' +
      '<div class="ac-fid-cell"><span>SHA-256 Checksums</span><strong style="font-family:monospace;font-size:.73rem">' + esc((fidelity.before_sha256 || '—').slice(0, 12)) + ' → ' + esc((fidelity.after_sha256 || '—').slice(0, 12)) + '</strong></div>' +
      '</div></div>'
    ) : '';

    body.innerHTML = pickerHtml + imageStageHtml + consHtml + pagesHtml + metaHtml + fidelityHtml;
    footer.innerHTML = '<div style="font-size:.76rem;color:rgb(var(--text-secondary))">Interactive Before &amp; After Studio · 100% Pure-Rust local analysis (0 LLM tokens)</div>';

    // Bind Compare Document button
    var docBtn = document.getElementById('ac-cmp-doc-btn');
    if (docBtn) {
      docBtn.onclick = async function () {
        var sel = document.getElementById('ac-cmp-doc-sel');
        var slug = sel && sel.value;
        if (!slug) { toast('Select a document first', false); return; }
        docBtn.disabled = true;
        docBtn.textContent = '⏳ Comparing…';
        var r = await acApi('/canvas/api/compare-document/' + encodeURIComponent(slug));
        docBtn.disabled = false;
        docBtn.textContent = 'Compare Original vs Latest';
        if (r.ok && r.data) {
          r.data.documents = docs;
          ac.data = r.data;
          renderCompareCanvas(body, footer);
        } else toast((r.data && r.data.reason) || 'Comparison failed', false);
      };
    }

    // Bind Compare Two Uploaded Files button
    var runBtn = document.getElementById('ac-cmp-run-btn');
    if (runBtn) {
      runBtn.onclick = async function () {
        var bIn = document.getElementById('ac-cmp-before-in');
        var aIn = document.getElementById('ac-cmp-after-in');
        var bf = bIn && bIn.files && bIn.files[0];
        var af = aIn && aIn.files && aIn.files[0];
        if (!bf || !af) { toast('Choose both a Before file and an After file', false); return; }
        runBtn.disabled = true;
        runBtn.textContent = '⏳ Analyzing…';
        var fd = new FormData();
        fd.append('before_file', bf);
        fd.append('after_file', af);
        var r = await acApi('/canvas/api/compare-analyze', { method: 'POST', body: fd });
        runBtn.disabled = false;
        runBtn.textContent = '⚖️ Compare Uploaded Files';
        if (r.ok && r.data) {
          r.data.documents = docs;
          ac.data = r.data;
          renderCompareCanvas(body, footer);
          toast('Before → After analysis complete');
        } else toast((r.data && r.data.reason) || 'Comparison failed', false);
      };
    }

    // View mode switches (Split Slider | Side-by-Side) & Diff mode switches (Side-by-Side Diff | Unified Diff)
    body.querySelectorAll('[data-ac-cv]').forEach(function (b) {
      b.onclick = function () {
        ac.compareView = b.getAttribute('data-ac-cv');
        renderCompareCanvas(body, footer);
      };
    });
    body.querySelectorAll('[data-ac-cdm]').forEach(function (b) {
      b.onclick = function () {
        ac.compareDiffMode = b.getAttribute('data-ac-cdm');
        renderCompareCanvas(body, footer);
      };
    });

    // Interactive Curtain Slider
    var cRange = document.getElementById('ac-curtain-range');
    var cBefore = document.getElementById('ac-curtain-before');
    var cLine = document.getElementById('ac-curtain-line');
    var cStage = document.getElementById('ac-curtain-stage');
    function setCurtain(pct) {
      ac.compareSliderPct = Math.max(0, Math.min(100, Math.round(pct)));
      if (cRange) cRange.value = String(ac.compareSliderPct);
      if (cBefore) cBefore.style.clipPath = 'inset(0 ' + (100 - ac.compareSliderPct) + '% 0 0)';
      if (cLine) cLine.style.left = ac.compareSliderPct + '%';
    }
    if (cRange) cRange.oninput = function (e) { setCurtain(Number(e.target.value)); };
    if (cStage) {
      var dragging = false;
      var updateFromEvent = function (e) {
        var rect = cStage.getBoundingClientRect();
        if (rect.width > 0) setCurtain(((e.clientX - rect.left) / rect.width) * 100);
      };
      cStage.addEventListener('mousedown', function (e) { dragging = true; updateFromEvent(e); });
      window.addEventListener('mousemove', function (e) { if (dragging) updateFromEvent(e); });
      window.addEventListener('mouseup', function () { dragging = false; });
    }

    // Overlay Blend Slider
    var bRange = document.getElementById('ac-blend-range');
    var bTop = document.getElementById('ac-blend-top');
    if (bRange && bTop) {
      bRange.oninput = function (e) {
        ac.compareBlendPct = Number(e.target.value);
        bTop.style.opacity = String(ac.compareBlendPct / 100);
      };
    }

    // Metadata Changed-only Toggle
    var mChk = document.getElementById('ac-meta-chg-only');
    if (mChk) {
      mChk.onchange = function () {
        ac.compareMetaChangedOnly = mChk.checked;
        renderCompareCanvas(body, footer);
      };
    }
  }

  function upgradeCanvasLinks() {
    var links = document.querySelectorAll('a[href*="#arkib-canvas:"]:not([data-ac-card="1"])');
    links.forEach(function (a) {
      var parsed = parseCanvasHash(a.getAttribute('href') || '');
      if (!parsed) return;
      a.setAttribute('data-ac-card', '1');
      var isOcr = parsed.mode === 'ocr';
      var isCmp = parsed.mode === 'compare';
      var title = isCmp ? 'Before & After Comparison Studio' : isOcr ? 'Jawi OCR Workbench' : 'Identity Enrollment Canvas';
      var sub = isCmp
        ? 'Interactive slider · Changed pages · Conservation & Migration Fidelity'
        : isOcr
          ? 'Edit Jawi & Rumi side-by-side · Export DOCX / PDF'
          : 'Crop photo, set group folder & enroll to archive';
      var iconSvg = isCmp
        ? '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="3" x2="12" y2="21"/><rect x="3" y="5" width="7" height="14" rx="1"/><rect x="14" y="5" width="7" height="14" rx="1"/></svg>'
        : isOcr
          ? '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/></svg>'
          : '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="2"/><path d="M15 8h2"/><path d="M15 12h2"/><path d="M7 16c.5-1.5 2-2 4-2s3.5.5 4 2"/></svg>';
      a.innerHTML =
        '<span class="ac-cc-left">' +
        '  <span class="ac-cc-icon">' + iconSvg + '</span>' +
        '  <span class="ac-cc-meta">' +
        '    <span class="ac-cc-title">' + title + '</span>' +
        '    <span class="ac-cc-sub">' + sub + '</span>' +
        '  </span>' +
        '</span>' +
        '<span class="ac-cc-open">Open ↗</span>';
    });
  }

  var acObserver = new MutationObserver(function () {
    mountCanvasButton();
    upgradeCanvasLinks();
  });
  acObserver.observe(document.documentElement, { childList: true, subtree: true });
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', function () { mountCanvasButton(); upgradeCanvasLinks(); });
  } else {
    mountCanvasButton();
    upgradeCanvasLinks();
  }

  // Passive chatbot prompt tracker for User Insights & Hot Topics (never blocks or alters chat)
  var _lastTrackedPrompt = '';
  var _lastTrackedAt = 0;
  function sendChatPromptAnalytics(rawText) {
    var txt = String(rawText || '').trim();
    var now = Date.now();
    if (txt.length < 2) return;
    if (txt === _lastTrackedPrompt && (now - _lastTrackedAt) < 4000) return;
    _lastTrackedPrompt = txt;
    _lastTrackedAt = now;
    try {
      fetch(API_BASE + '/canvas/api/track-chat-query', {
        method: 'POST',
        credentials: 'include',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ prompt: txt.slice(0, 280) })
      }).catch(function () {});
    } catch (_) {}
  }
  document.addEventListener('keydown', function (ev) {
    if (ev.key === 'Enter' && !ev.shiftKey && !ev.isComposing) {
      var t = ev.target;
      if (t && (t.id === 'prompt-textarea' || (t.tagName === 'TEXTAREA' && t.closest && t.closest('form')))) {
        if (!t.closest('#rf-admin-stage') && !t.closest('#ac-drawer')) {
          sendChatPromptAnalytics(t.value || t.textContent || '');
        }
      }
    }
  }, true);
  document.addEventListener('click', function (ev) {
    var btn = ev.target && ev.target.closest && ev.target.closest('button[data-testid="send-button"], button[type="submit"]');
    if (!btn || btn.closest('#rf-admin-stage') || btn.closest('#ac-drawer')) return;
    var form = btn.closest('form');
    if (!form) return;
    var ta = form.querySelector('#prompt-textarea, textarea');
    if (ta) sendChatPromptAnalytics(ta.value || ta.textContent || '');
  }, true);

  // Do NOT auto-reopen stale session text on a fresh /c/new page reload; clear stale hash if on /c/new
  var initParsed = parseCanvasHash(window.location.hash || '');
  if (initParsed) {
    if (window.location.pathname === '/c/new' || window.location.pathname === '/') {
      try { history.replaceState(null, '', window.location.pathname + window.location.search); } catch (_) {}
    } else {
      setTimeout(function () { openCanvas(initParsed.mode, initParsed.id, true); }, 450);
    }
  }
})();
