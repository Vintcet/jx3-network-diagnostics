"""Exercise real Tauri IPC in this application's own WebView2 instance."""
import json
import gzip
import os
from pathlib import Path
import socket
import subprocess
import time
import urllib.request
from playwright.sync_api import sync_playwright, expect

root = Path(__file__).resolve().parents[1]
out = root / '.tmp' / 'desktop-check'
out.mkdir(parents=True, exist_ok=True)
exe = root / 'src-tauri' / 'target' / 'debug' / 'jx3-network-diagnostics.exe'
with socket.socket() as sock:
    sock.bind(('127.0.0.1', 49387))
    port = sock.getsockname()[1]
env = os.environ.copy()
env['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = f'--remote-debugging-port={port} --remote-debugging-address=127.0.0.1'
env['WEBVIEW2_USER_DATA_FOLDER'] = str(out / 'webview-profile')
local_http = urllib.request.build_opener(urllib.request.ProxyHandler({}))
startup = subprocess.STARTUPINFO()
startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
startup.wShowWindow = 0
process = subprocess.Popen([str(exe)], cwd=root, env=env, startupinfo=startup, creationflags=subprocess.CREATE_NO_WINDOW)
try:
    deadline = time.monotonic() + 40
    while time.monotonic() < deadline:
        try:
            local_http.open(f'http://127.0.0.1:{port}/json/version', timeout=1).close()
            break
        except Exception as error:
            last_error = str(error)
            if process.poll() is not None:
                raise RuntimeError(f'App exited with {process.returncode}')
            time.sleep(.3)
    else:
        raise RuntimeError(f'WebView2 debugging port did not start: {last_error}')
    with sync_playwright() as p:
        browser = p.chromium.connect_over_cdp(f'http://127.0.0.1:{port}')
        context = browser.contexts[0]
        deadline = time.monotonic() + 20
        while not context.pages and time.monotonic() < deadline:
            time.sleep(.2)
        page = context.pages[0]
        errors = []
        page.on('pageerror', lambda error: errors.append(str(error)))
        page.wait_for_load_state('domcontentloaded')
        page.get_by_role('dialog', name='开始诊断前的小提示').get_by_role('button', name='关闭', exact=True).click()
        expect(page.get_by_role('button', name='开始测试', exact=True)).to_be_enabled(timeout=20000)
        expect(page.get_by_role('heading', name='自动跟随游戏连接')).to_be_visible()
        expect(page.locator('.app-meta')).to_contain_text('兰舟少住')
        page.screenshot(path=str(out / 'ready.png'), full_page=True)
        # JS communicates through the very same IPC implementation used by the UI.
        processes = page.evaluate("window.__TAURI_INTERNALS__.invoke('get_processes')")
        own = next(p for p in processes if p['pid'] == process.pid)
        assert own['path'].lower() == str(exe).lower()
        assert isinstance(own['started'], str), 'FILETIME must not lose precision through JS numbers'
        page.get_by_role('combobox', name='关联游戏进程').select_option(f"{own['pid']}:{own['started']}")
        page.get_by_role('combobox', name='测试时长').select_option('custom')
        page.get_by_role('spinbutton', name='自定义分钟数').fill('1')
        page.get_by_role('button', name='开始测试', exact=True).click()
        expect(page.get_by_role('button', name='结束并分析', exact=True)).to_be_enabled(timeout=10000)
        install_error = page.evaluate("""async () => {
            try { await window.__TAURI_INTERNALS__.invoke('install_update', {onProgress: '__CHANNEL__:999999'}); return ''; }
            catch (e) { return String(e); }
        }""")
        assert '请先结束测试' in install_error or '更新操作正在进行' in install_error, install_error
        for _ in range(60):
            measured = page.evaluate("window.__TAURI_INTERNALS__.invoke('get_session')")
            if len(measured['probes']) > 4:
                break
            page.wait_for_timeout(250)
        else:
            raise AssertionError('No real probe samples reached the desktop within 15 seconds')
        page.get_by_role('button', name='刚刚卡了', exact=True).click()
        expect(page.get_by_text('已记录卡顿标记，将与前后测量数据一起分析。')).to_be_visible()
        page.screenshot(path=str(out / 'running.png'), full_page=True)
        page.get_by_role('button', name='联网程序', exact=True).click()
        page.get_by_role('textbox', name='搜索联网进程').fill(str(process.pid))
        row = page.locator('button.process-row').filter(has_text='jx3-network-diagnostics.exe')
        expect(row).to_be_visible(timeout=10000)
        row.click()
        expect(page.get_by_role('textbox', name='jx3-network-diagnostics.exe 完整路径')).to_have_value(str(exe))
        page.screenshot(path=str(out / 'processes.png'), full_page=True)
        page.get_by_role('button', name='实时诊断', exact=True).click()
        page.get_by_role('button', name='结束并分析', exact=True).click()
        expect(page.get_by_role('heading', name='诊断报告', exact=True)).to_be_visible(timeout=15000)
        view = page.evaluate("window.__TAURI_INTERNALS__.invoke('get_session')")
        assert view['status'] == 'stopped', view
        assert any(e['kind'] == 'marker' for e in view['report']['events'])
        log_dir = Path(view['logDir'])
        with gzip.open(log_dir / 'samples.jsonl.gz', 'rt', encoding='utf-8') as stream:
            records = [json.loads(line) for line in stream]
        assert {'sample', 'probe', 'event'} <= {r['type'] for r in records}
        assert (log_dir / 'report.html').exists()
        assert (log_dir / '诊断报告.md').exists()
        assert (log_dir / '本次测试说明.txt').exists()
        assert view['report']['server']['name'] in log_dir.name
        (out / 'result.json').write_text(json.dumps({'status': view['status'], 'logDir': view['logDir'], 'records': len(records), 'traffic': view['report']['trafficStatus'], 'errors': errors}, ensure_ascii=False, indent=2), encoding='utf-8')
        page.screenshot(path=str(out / 'report.png'), full_page=True)
        page.get_by_role('button', name='测试记录', exact=True).click()
        expect(page.get_by_role('button', name='查看报告').first).to_be_enabled(timeout=10000)
        page.get_by_role('button', name='查看报告').first.click()
        expect(page.get_by_role('heading', name='诊断报告', exact=True)).to_be_visible()
        expect(page.get_by_text('先看结论', exact=True)).to_be_visible()
        saved = page.evaluate("window.__TAURI_INTERNALS__.invoke('get_history')")
        previous = next((h for h in saved if h['id'] != view['id'] and h['status'] == 'completed' and h['durationSeconds'] >= 60), None)
        if previous:
            restored = page.evaluate("id => window.__TAURI_INTERNALS__.invoke('get_report', {id})", previous['id'])
            assert restored['summary']['headline'] and restored['summary']['facts']
            exported = page.evaluate("id => window.__TAURI_INTERNALS__.invoke('export_report', {id})", previous['id'])
            markdown = Path(exported).read_text(encoding='utf-8')
            assert '实际异常阈值' in markdown and '基线' in markdown
            (out / 'restored-report.json').write_text(json.dumps(restored, ensure_ascii=False, indent=2), encoding='utf-8')
            page.get_by_role('button', name='测试记录', exact=True).click()
            index = next(i for i, item in enumerate(saved) if item['id'] == previous['id'])
            page.locator('.history-row').nth(index).get_by_role('button', name='查看报告').click()
            expect(page.get_by_text(restored['summary']['headline'], exact=True)).to_be_visible()
            page.screenshot(path=str(out / 'restored-report.png'), full_page=True)
        assert not errors, errors
        # Send a normal WM_CLOSE so the app runs its own shutdown/ETW cleanup.
        import ctypes
        from ctypes import wintypes
        user32 = ctypes.windll.user32
        user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
        user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
        user32.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
        closed = []
        def close_owned(hwnd, _):
            pid = wintypes.DWORD()
            user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
            if pid.value == process.pid:
                title = ctypes.create_unicode_buffer(256)
                user32.GetWindowTextW(hwnd, title, len(title))
                if '剑网三网络诊断' in title.value:
                    assert user32.PostMessageW(hwnd, 0x0010, 0, 0)
                    closed.append(title.value)
            return True
        cb = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)(close_owned)
        user32.EnumWindows(cb, 0)
        assert closed, 'The application main window was not found for normal shutdown'
        process.wait(timeout=12)
        browser.close()
finally:
    if process.poll() is None:
        process.terminate()
        process.wait(timeout=8)
print('Real desktop IPC: process identity/path, start, sampling, marker, process detail, early stop, JSONL, report and history passed.')
