"""Browser shell checks. Live Windows IPC is tested separately against the desktop app."""
from pathlib import Path
import json
from playwright.sync_api import sync_playwright, expect

root = Path(__file__).resolve().parents[1]
app_info = json.loads((root / 'package.json').read_text(encoding='utf-8'))
out = root / '.tmp' / 'ui-check'
out.mkdir(parents=True, exist_ok=True)

with sync_playwright() as p:
    browser = p.chromium.launch(channel='msedge', headless=True)
    page = browser.new_page(viewport={'width': 1360, 'height': 1000}, device_scale_factor=1)
    errors = []
    page.on('pageerror', lambda error: errors.append(str(error)))
    page.goto('http://127.0.0.1:5173')
    page.wait_for_load_state('networkidle')
    page.screenshot(path=str(out / 'startup-notice.png'))
    page.get_by_role('dialog', name='开始诊断前的小提示').get_by_role('button', name='关闭', exact=True).click()
    expect(page.get_by_role('button', name='开始测试', exact=True)).to_be_disabled()
    expect(page.get_by_text('当前为浏览器界面预览。', exact=False)).to_be_visible()
    expect(page.locator('.app-meta')).to_contain_text('兰舟少住')
    expect(page.locator('.app-meta time')).to_have_text(app_info['releaseDate'])
    expect(page.get_by_role('button', name='检查更新', exact=True)).to_be_disabled()
    page.screenshot(path=str(out / 'desktop-empty.png'), full_page=True)
    page.get_by_role('button', name='联网程序', exact=True).click()
    expect(page.get_by_role('heading', name='本机联网程序')).to_be_visible()
    page.get_by_role('button', name='诊断报告', exact=True).click()
    expect(page.get_by_role('heading', name='测试结束后，在这里查看分析')).to_be_visible()
    page.get_by_role('button', name='测试记录', exact=True).click()
    expect(page.get_by_role('heading', name='本机测试记录')).to_be_visible()
    page.get_by_role('button', name='使用说明', exact=True).click()
    expect(page.get_by_role('heading', name='怎样读懂数据')).to_be_visible()
    expect(page.get_by_role('link', name='https://github.com/Vintcet/jx3-network-diagnostics')).to_be_visible()
    assert page.locator('.guide > section').first.locator(':scope > :first-child').get_attribute('class') == 'source-link'
    page.get_by_role('button', name='实时诊断', exact=True).click()
    page.get_by_role('combobox', name='测试时长').select_option('custom')
    page.get_by_role('spinbutton', name='自定义分钟数').fill('0')
    expect(page.get_by_role('button', name='开始测试', exact=True)).to_be_disabled()
    for width in (1000, 768, 375):
        page.set_viewport_size({'width': width, 'height': 900})
        page.emulate_media(reduced_motion='reduce')
        assert page.evaluate('document.documentElement.scrollWidth <= window.innerWidth'), f'horizontal overflow at {width}'
        page.screenshot(path=str(out / f'width-{width}.png'), full_page=True)
    assert not errors, errors
    browser.close()
print('Browser shell: navigation, empty states, duration validation, responsive layout and console passed.')
