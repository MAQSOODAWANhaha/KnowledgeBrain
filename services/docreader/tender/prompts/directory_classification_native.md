本次 operation=classify_with_native_requirement_cases_v1 使用单一 q 来源池。完整 documents 的行使用全源短ref；items 只返回 kind 和同文件完整 range=[首ref,尾ref]，每文件每kind最多一项。basis_groups 是共享的真实来源依据，每组 ranges 只选完整短ref区间；所有组的区间总数不超过 case 数，每个 decision 仅以 basis_id 引用组。没有额外依据使用 null，旧 basis_windows 留空；not_applicable 必须有实际依据。

requirement_cases 中 legacy 的窗口仍以完整 documents 原文为准；native 的 body 与池化 native_contexts 来自精确原格或原字，禁止用平铺 documents 的混列行替换。所有 q 恰处置一次，chapter_ids 只选 c 短身份。不同 source_kind 不合并成一项；不能为减少未决混淆来源权威。单 native case 保持原 source_join，多 case 只有同一完整语义条款才显式续接；保全上下文、条件、例外及共同后果，不能把不同材料义务强并。不同候选 owner 由本次实际章节职责明确核定；没有完整证据继续 unassigned。manual_only 来源须单独保留为 unassigned，交编制人员对照原件核定。

封面不能承担章内要求；已核定不适用源窗不得再分配章内。document 仅为整稿指导，procedure 为外部待执行动作；它们不代表企业已符合。几何未确认的 legacy 来源不能提升正文。不能按关键词补词、删除数值，或用没有企业资料作为不适用依据。
