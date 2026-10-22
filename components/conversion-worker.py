"""拾光离线转换适配器；由桌面端启动，不联网，不安装组件。"""
import pathlib
import subprocess
import sys
import time


def main():
    source, output, libreoffice, workspace = sys.argv[1:5]
    source, output, workspace = map(pathlib.Path, (source, output, workspace))
    deadline = time.monotonic() + 15
    while not (workspace / "ready").exists():
        if time.monotonic() > deadline:
            raise RuntimeError("桌面端未完成进程隔离，已停止转换")
        time.sleep(0.05)
    if source.suffix.lower() == ".pdf":
        try:
            from pdf2docx import Converter
        except ImportError as error:
            raise RuntimeError("当前 Python 环境缺少 pdf2docx，请在设置中重新下载或检查转换组件") from error
        converter = Converter(str(source))
        try:
            converter.convert(str(output), multi_processing=False)
        finally:
            converter.close()
    else:
        executable = pathlib.Path(libreoffice)
        if not executable.is_absolute() or not executable.is_file():
            raise RuntimeError("LibreOffice 路径无效，请配置 soffice.com 或 soffice.exe 的绝对路径")
        export = workspace / "export"
        export.mkdir(exist_ok=True)
        profile = (workspace / "libreoffice-profile").resolve().as_uri()
        result = subprocess.run(
            [str(executable), "-env:UserInstallation=" + profile, "--headless", "--norestore",
             "--convert-to", "pdf:writer_pdf_Export", "--outdir", str(export), str(source)],
            stdin=subprocess.DEVNULL, check=False,
            creationflags=0x08000000 if sys.platform == "win32" else 0,
        )
        generated = export / (source.stem + ".pdf")
        if result.returncode or not generated.is_file():
            raise RuntimeError("LibreOffice 未生成 PDF，请检查文件是否损坏、受密码保护或被占用")
        generated.replace(output)
    if not output.is_file() or output.stat().st_size == 0:
        raise RuntimeError("组件未生成有效输出")


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(str(error), file=sys.stderr, flush=True)
        sys.exit(1)
