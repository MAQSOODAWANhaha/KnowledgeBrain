本次 operation 为 select_directory_refs_v2，以下规则补充基础目录选择规则；阅读导航的去向使用本次新增的 reading_only_refs。

reading_only_refs 只放完整已给原文中非空、可见的纯阅读导航锚点，例如格式目录、组成说明，以及结合完整上下文确实没有实质交付要求的空补充栏目。只选择 ref，不誊写导航正文，也无需为导航创建 body_ranges:[] 的空 node。每个 ref 只列一次，且不能与本计划任何 node.heading_ref 相同。宿主单独保留这些阅读来源，不把它们作为投标正文、已满足义务或材料排除依据。

source_only 保持原义：它只能引用本计划 nodes 中已经声明的真实 heading_ref，表示该来源节点明确不采用为本次应答正文的去向。parent_ref、scope.items.node_ref、excluded.node_ref、source_only 和 format_bindings 中的所有节点引用，都必须指向本计划已声明节点。不要把仅在原文可见、但未在 nodes 声明的 ref 当成 node 选择。

若某个 excluded.node_ref 尚未声明，回到原文判断：有真实原式或实质材料时，声明真实 heading_ref、完整 body_ranges 和必要的主体/阶段条件，再按真实排除依据处理；只有确实不构成材料的纯阅读导航才放 reading_only_refs。不要自动删除错误引用、造空 node 或把所有未采用材料一律改成阅读导航。实际独立原式即使不属于本次提交，也应保全原式及适用条件，或给出准确限定该材料、主体和阶段的排除依据。

reading_only_refs 不结算任何已声明节点的去向，也不结算组成清单条目。每个真实 node 仍须采用、给 excluded 的真实来源依据，或明确 source_only；组成中的独立材料仍须有独立采用或排除依据。明确要求投标人编写技术方案、计划或提交支持材料的，即使格式页只有标题、body_ranges 为空，也要保留真实 narrative/attachment 材料节点、具体组成依据及未确认条件，不能降格为阅读导航。父子原式仍分别选择各自正文，不重复拥有同一材料正文。

两份计划仍独立选择完整已给原文，不按另一计划补机械空图；只返回本次 v2 Schema 的 JSON，不誊写文件身份、UUID、原文或企业事实。未知节点反馈中的 path/ref 用于修正该具体字段；reading_only_refs 只解决阅读角色，不替代真实材料判断。
