import { useEffect, useRef } from 'react';
import { Info } from 'lucide-react';
import './startup-notice.css';

export function StartupNotice() {
  const dialogRef = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = dialogRef.current;
    dialog?.showModal();
    return () => dialog?.close();
  }, []);

  return <dialog className="startup-dialog" ref={dialogRef} aria-labelledby="startup-title" aria-describedby="startup-description">
    <div className="startup-heading">
      <Info size={24} aria-hidden="true" />
      <h2 id="startup-title">开始诊断前的小提示</h2>
    </div>
    <div className="startup-body">
      <p id="startup-description">建议按以下方式进行测试，便于获取更完整、易于分析的诊断信息。</p>
      <ol>
        <li>
          <h3>建议先关闭加速器</h3>
          <p>关闭游戏加速器后再进行测试，便于观察直连网络情况，减少加速线路对诊断结果的影响。</p>
        </li>
        <li>
          <h3>建议以管理员身份运行</h3>
          <p>右键点击本工具的程序或快捷方式 → 选择“以管理员身份运行”，有助于读取更完整的进程与网络数据。</p>
        </li>
        <li>
          <h3>诊断结果可以导出给 AI</h3>
          <p>测试结束后，在报告页点击“导出给 AI 的报告”，将生成的“诊断报告.md”上传或复制给 AI，辅助分析问题。</p>
        </li>
      </ol>
    </div>
    <div className="startup-actions">
      <button type="button" className="button primary" autoFocus onClick={() => dialogRef.current?.close()}>关闭</button>
    </div>
  </dialog>;
}
