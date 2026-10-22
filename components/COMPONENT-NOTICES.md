# 拾光可选转换组件

组件在用户点击“自动下载组件”后优先从发布者的 HTTPS 地址单独下载，不包含在拾光安装包中。7-Zip 的固定官方地址不可达时，使用 gh-proxy.com 第三方公开文件转发作为备用传输；该服务不是官方镜像。主地址和备用地址必须通过同一份固定版本、大小和 SHA-256 校验，不接受其他版本或跳过校验。具体地址见 download-manifest.json。下载全部校验通过后，在拾光数据目录解包并自动配置，不更改系统 Python、PATH、文件关联或已安装的 LibreOffice。

## 来源和许可

- LibreOffice Portable 26.2.4 Standard：PortableApps.com 发布、The Document Foundation 镜像分发。LibreOffice 使用 MPL-2.0 等许可，附带其他组件的许可。保留便携包内的许可和版权文件。来源：https://portableapps.com/apps/office/libreoffice_portable；源码：https://www.libreoffice.org/download-other/。
- Python 3.13.16 嵌入式环境：Python Software Foundation 官方下载，PSF-2.0，保留包内 LICENSE。来源：https://www.python.org/downloads/release/python-31316/。
- 7-Zip 26.05 与 7zr：Igor Pavlov 官方发行包。7-Zip 使用 LGPL-2.1 及发行包所列附加条款，保留 License.txt、readme.txt 等文件；7zr 发布为 Public domain。仅在工具数据目录解包、读取 LibreOffice 便携发行包，不运行其安装器，不注册系统级 7-Zip 或文件关联。来源与源码：https://www.7-zip.org/；https://github.com/ip7z/7zip/releases/tag/26.05。
- pip：MIT；pdf2docx 0.5.13：MIT；python-docx：MIT；fonttools：MIT；fire：Apache-2.0；termcolor：MIT；typing_extensions：PSF-2.0；lxml：BSD-3-Clause 及附带库许可；NumPy：BSD-3-Clause 及附带库许可；OpenCV headless：Apache-2.0 及附带库许可。具体版本与原始 PyPI 许可元数据见清单，原发行包的 dist-info / licenses 文件保留在组件环境内。
- PyMuPDF 1.28.2 采用 AGPL-3.0 或 Artifex 商业许可，免费使用受其开源条款约束，不能将“免费下载”理解为没有许可条件。来源与源码：https://pypi.org/project/PyMuPDF/1.28.2/；https://github.com/pymupdf/PyMuPDF。

## 分发说明

拾光源码采用负责人选定的 AGPL-3.0-only，完整项目协议见根目录 LICENSE。项目源码与可选转换组件分别提供；第三方组件仍按各自许可使用和分发。修改、重新分发或合并打包转换环境时，应保留原版权、协议及对应源码来源。本说明不是对第三方原始许可文本的替代。

运行转换时只在本机处理文件；下载阶段只请求组件文件，不上传用户文档。PDF 转 Word 不包含扫描件识字，复杂排版仍可能失真。固定清单不会在后台自动升级依赖；维护者更新清单时需重新核对兼容性、哈希、许可和已知安全公告。
