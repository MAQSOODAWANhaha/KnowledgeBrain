# R1 Rust镜像构建来源修复

`deploy/Dockerfile.rust`原来只向Rust builder复制queue registry和数据库初始化脚本，没有复制平台编译期嵌入的三个合同文件：

- `deploy/catalog-manifest-v1.schema.json`
- `deploy/platform-frozen-seed-tables-v2.json`
- `deploy/release-descriptor-v1.schema.json`

仓库本地编译会读取已有文件，因此此前本地检查通过不能证明Docker builder可编译。按原Dockerfile的Rust阶段COPY输入重建隔离目录后，`cargo check -p platform --lib`实际以101退出，分别报告上述三处`include_str!`文件不存在。

现已在原Dockerfile中显式复制这三个既有文件。相同隔离输入补齐后，API、Worker、Retention、Platform的全部binary目标`cargo check`以0退出，使用Rust1.97、`--locked --offline`和独立target目录。没有复制`.env`、新增schema或migration，也没有启动容器或操作数据库。

证据见[verification.json](../../artifacts/release-build-context/verification.json)：保留失败/通过日志、修改前后Dockerfile、完整隔离源文件摘要清单。阶段目录位于`/tmp/kb-release-context.mgmjoqf7/`。

本次只验证重建Rust COPY输入后的宿主编译检查。没有执行Docker整镜像构建、Node构建、镜像拉取/发布、release链接、RepoDigest验证、启动或数据库receipt/readiness验收。R1发布工具 `knowledgebrain-release` 和整项隔离验收仍待完成；此修复也不替代真实招标Agent整稿验收。
