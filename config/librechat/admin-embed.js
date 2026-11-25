(function () {
  'use strict';

  var BTN_ID = 'rf-admin-rail-btn';
  var STAGE_ID = 'rf-admin-stage';
  var FRAME_ID = 'rf-admin-frame';
  var isOpen = false;
  var railEl = null;

  function isDarkMode() {
    return document.documentElement.classList.contains('dark');
  }

  function adminUrl() {
    var host = window.location.hostname || 'localhost';
    var theme = isDarkMode() ? 'dark' : 'light';
    return 'http://' + host + ':8005/admin?embed=1&theme=' + theme;
  }

  function syncThemeToFrame() {
    var stage = document.getElementById(STAGE_ID);
    var dark = isDarkMode();
    if (stage) {
      stage.style.backgroundColor = dark ? '#0f172a' : '#ffffff';
    }
    var frame = document.getElementById(FRAME_ID);
    if (frame && frame.contentWindow) {
      try {
        frame.contentWindow.postMessage({ type: 'rojakface-theme', dark: dark }, '*');
      } catch (e) {}
    }
  }

  function computeRailRight() {
    if (railEl && document.body.contains(railEl)) {
      var rect = railEl.getBoundingClientRect();
      if (rect.width >= 36 && rect.width <= 96) {
        return Math.round(rect.right);
      }
    }
    return 52;
  }

  function positionStage() {
    var stage = document.getElementById(STAGE_ID);
    if (!stage) return;
    stage.style.left = computeRailRight() + 'px';
  }

  function ensureStage() {
    var stage = document.getElementById(STAGE_ID);
    if (stage) return stage;
    stage = document.createElement('div');
    stage.id = STAGE_ID;
    stage.setAttribute('role', 'region');
    stage.setAttribute('aria-label', 'RojakFace Admin Portal');
    stage.style.display = 'none';
    stage.style.backgroundColor = isDarkMode() ? '#0f172a' : '#ffffff';

    var frame = document.createElement('iframe');
    frame.id = FRAME_ID;
    frame.title = 'RojakFace Admin Portal';
    frame.setAttribute('allow', 'clipboard-read; clipboard-write');
    frame.addEventListener('load', syncThemeToFrame);
    stage.appendChild(frame);
    document.body.appendChild(stage);
    return stage;
  }

  function setButtonActive(active) {
    var btn = document.getElementById(BTN_ID);
    if (!btn) return;
    btn.classList.toggle('rf-active', active);
    btn.setAttribute('aria-pressed', active ? 'true' : 'false');
  }

  function openAdmin() {
    var closeDrawerBtn = document.querySelector('[data-testid="close-sidebar-button"]');
    if (closeDrawerBtn) {
      try { closeDrawerBtn.click(); } catch (e) {}
    }
    var stage = ensureStage();
    var frame = document.getElementById(FRAME_ID);
    if (frame && !frame.getAttribute('src')) {
      frame.setAttribute('src', adminUrl());
    }
    isOpen = true;
    positionStage();
    syncThemeToFrame();
    stage.style.display = 'block';
    setButtonActive(true);
    setTimeout(positionStage, 120);
  }

  function closeAdmin() {
    if (!isOpen) return;
    isOpen = false;
    var stage = document.getElementById(STAGE_ID);
    if (stage) stage.style.display = 'none';
    setButtonActive(false);
  }

  function toggleAdmin(e) {
    if (e) {
      e.preventDefault();
      e.stopPropagation();
    }
    if (isOpen) {
      closeAdmin();
    } else {
      openAdmin();
    }
  }

  function createAdminButton() {
    var btn = document.createElement('button');
    btn.type = 'button';
    btn.id = BTN_ID;
    btn.setAttribute('data-testid', 'nav-panel-rojakface-admin');
    btn.setAttribute('aria-label', 'Admin Portal');
    btn.setAttribute('aria-pressed', isOpen ? 'true' : 'false');
    btn.setAttribute('title', 'Admin Portal');
    btn.className = 'rf-admin-rail-btn flex h-9 w-9 items-center justify-center rounded-lg transition-colors' + (isOpen ? ' rf-active' : '');
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
    btn.addEventListener('click', toggleAdmin);
    return btn;
  }

  function mountAdminButton() {
    var mcpBtn = document.querySelector('[data-testid="nav-panel-mcp-builder"]');
    if (!mcpBtn || !mcpBtn.parentElement) return;
    railEl = mcpBtn.closest('.bg-surface-sidebar') || mcpBtn.parentElement.parentElement;

    if (document.getElementById(BTN_ID)) {
      var existing = document.getElementById(BTN_ID);
      if (existing.previousElementSibling !== mcpBtn) {
        mcpBtn.insertAdjacentElement('afterend', existing);
      }
      return;
    }

    var adminBtn = createAdminButton();
    mcpBtn.insertAdjacentElement('afterend', adminBtn);
    if (isOpen) positionStage();
  }

  // Close embedded Admin Portal when user clicks any other LibreChat left-rail button
  document.addEventListener(
    'click',
    function (ev) {
      if (!isOpen) return;
      var target = ev.target && ev.target.closest ? ev.target.closest('button, a') : null;
      if (!target || target.id === BTN_ID) return;
      if (
        target.getAttribute('data-testid') &&
        (target.getAttribute('data-testid').indexOf('nav-panel-') === 0 ||
          target.getAttribute('data-testid') === 'new-chat-button' ||
          target.getAttribute('data-testid') === 'open-sidebar-button')
      ) {
        closeAdmin();
      }
    },
    true
  );

  // Escape key returns to chat
  document.addEventListener('keydown', function (ev) {
    if (ev.key === 'Escape' && isOpen) closeAdmin();
  });

  window.addEventListener('resize', function () {
    if (isOpen) positionStage();
  });

  // Watch DOM for LibreChat React rail rendering
  var domObserver = new MutationObserver(mountAdminButton);
  domObserver.observe(document.documentElement, { childList: true, subtree: true });

  // Watch LibreChat Light/Dark theme toggles and sync live into the Admin Portal
  var themeObserver = new MutationObserver(syncThemeToFrame);
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['class', 'style']
  });

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', mountAdminButton);
  } else {
    mountAdminButton();
  }

  // Support direct link http://localhost:3080/#rojakface-admin
  if (window.location.hash === '#rojakface-admin') {
    setTimeout(openAdmin, 500);
  }
})();
