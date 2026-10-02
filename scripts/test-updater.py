"""UI update flows with controlled IPC; no real installer is launched."""
from pathlib import Path
from playwright.sync_api import sync_playwright, expect

root = Path(__file__).resolve().parents[1]
out = root / '.tmp' / 'updater-check'
out.mkdir(parents=True, exist_ok=True)

mock = r"""
window.isTauri = true;
window.testSessionStatus = 'idle';
window.updateChecks = 0;
window.installAttempts = 0;
window.updateResult = {version: '0.5.0', notes: '测试更新说明'};
window.__TAURI_INTERNALS__ = {
  transformCallback: () => 1,
  unregisterCallback: () => {},
  invoke: async (command, args) => {
    if (command === 'check_update') {
      window.updateChecks++;
      if (window.failCheck) throw new Error('网络不可用，请重试');
      return window.updateResult;
    }
    if (command === 'install_update') {
      window.installAttempts++;
      args.onProgress.onmessage({phase: 'downloading', downloaded: 1048576, total: 2097152});
      await new Promise(resolve => setTimeout(resolve, 300));
      if (window.installAttempts === 1) throw new Error('签名校验失败，尚未安装更新');
      args.onProgress.onmessage({phase: 'installing', downloaded: 2097152, total: 2097152});
      return;
    }
    if (command === 'get_session') return {id: null, status: window.testSessionStatus, elapsed: 0, durationSeconds: 600, logDir: null, tick: null, probes: [], events: [], hops: [], report: null, error: null};
    if (command === 'get_catalog') return {servers: [{id:'test',name:'测试区服',area:'电信区',aliases:[],endpoints:[]}], source: '测试目录', fetchedAt: ''};
    if (command === 'get_processes' || command === 'get_history') return [];
    if (command === 'get_environment') return null;
    if (command === 'open_repository') { window.openedRepository = true; return; }
    throw new Error(`Unexpected IPC: ${command}`);
  }
};
"""

with sync_playwright() as p:
    browser = p.chromium.launch(channel='msedge', headless=True)
    page = browser.new_page(viewport={'width': 1360, 'height': 900})
    errors = []
    page.on('pageerror', lambda error: errors.append(str(error)))
    page.add_init_script(mock)
    page.goto('http://127.0.0.1:5173')
    expect(page.get_by_text('新版本 v0.5.0', exact=True)).to_be_visible(timeout=8000)
    assert page.evaluate('window.updateChecks') == 1, 'Startup should check once'
    page.get_by_text('查看更新内容', exact=True).click()
    expect(page.get_by_text('测试更新说明', exact=True)).to_be_visible()
    page.screenshot(path=str(out / 'available.png'), full_page=True)

    page.evaluate("window.testSessionStatus = 'running'")
    expect(page.get_by_role('button', name='下载并安装', exact=True)).to_be_disabled(timeout=5000)
    page.evaluate("window.testSessionStatus = 'stopping'")
    expect(page.locator('.session-status')).to_contain_text('正在结束并保存', timeout=5000)
    expect(page.get_by_role('button', name='下载并安装', exact=True)).to_be_disabled()
    assert page.evaluate('window.installAttempts') == 0
    page.evaluate("window.testSessionStatus = 'idle'")
    expect(page.get_by_role('button', name='下载并安装', exact=True)).to_be_enabled(timeout=5000)
    page.get_by_role('button', name='下载并安装', exact=True).click()
    expect(page.get_by_role('progressbar', name='更新下载进度')).to_have_attribute('value', '50')
    expect(page.get_by_role('button', name='开始测试', exact=True)).to_be_disabled()
    expect(page.get_by_text('Error: 签名校验失败，尚未安装更新')).to_be_visible()
    expect(page.get_by_role('button', name='开始测试', exact=True)).to_be_enabled()
    page.get_by_role('button', name='重试下载安装', exact=True).click()
    expect(page.get_by_text('签名校验通过，正在启动安装程序…')).to_be_visible()
    assert page.evaluate('window.installAttempts') == 2
    expect(page.get_by_role('button', name='开始测试', exact=True)).to_be_disabled()
    page.screenshot(path=str(out / 'installing.png'), full_page=True)

    page.reload()
    page.evaluate('window.failCheck = true')
    expect(page.get_by_role('button', name='检查失败，重试')).to_be_visible(timeout=8000)
    expect(page.get_by_role('region', name='软件更新')).not_to_be_visible()
    page.get_by_role('button', name='检查失败，重试').click()
    expect(page.get_by_text('Error: 网络不可用，请重试')).to_be_visible()
    page.evaluate('window.failCheck = false; window.updateResult = null')
    page.get_by_role('button', name='重试检查', exact=True).click()
    expect(page.get_by_text('已是最新版本', exact=True)).to_be_visible()
    page.get_by_role('button', name='使用说明', exact=True).click()
    page.get_by_role('link', name='https://github.com/Vintcet/jx3-network-diagnostics').click()
    assert page.evaluate('window.openedRepository')
    for width in (1360, 1000, 768, 375):
        page.set_viewport_size({'width': width, 'height': 720})
        assert page.evaluate('document.documentElement.scrollWidth <= innerWidth')
        if width >= 1000:
            expect(page.locator('.app-meta time')).to_be_in_viewport()
        page.screenshot(path=str(out / f'guide-{width}.png'), full_page=True)
    assert not errors, errors
    browser.close()
print('Updater UI passed: automatic check, progress, errors/retry, active-test guard, start lock, source link and layout.')
