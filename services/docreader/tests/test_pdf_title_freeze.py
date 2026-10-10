"""gaps-1 回归测试：冻结标题两规则（短标题横幅让位、短索引续写池）。"""

import unittest

from docreader.parser.pdf_parser import (
    _demote_banner_headings,
    _drop_repeating_lines,
    _merge_short_index_continuations,
    _sectionize_pages,
)


class DemoteBannerHeadingsTest(unittest.TestCase):
    def test_banner_heading_is_demoted_before_drop(self):
        # 版式识别把重复横幅标成了标题；不降级的话 "# " 前缀会让它
        # 逃过 repeating 集合匹配，每页冻结出一个伪章节。
        texts = ["# 投标文件\n第一章\n内容", "# 投标文件\n第二章\n内容"]
        classes = ["text", "text"]
        repeating = {"投标文件"}
        demoted = _demote_banner_headings(texts, classes, repeating)
        self.assertEqual(demoted[0].splitlines()[0], "投标文件")
        self.assertEqual(demoted[1].splitlines()[0], "投标文件")
        # 降级后 drop 能正常清除横幅行。
        cleaned = _drop_repeating_lines(demoted, classes, repeating)
        self.assertEqual(cleaned[0], "第一章\n内容")
        self.assertEqual(cleaned[1], "第二章\n内容")

    def test_body_title_not_in_repeating_set_is_kept(self):
        texts = ["# 第一章 总则\n内容"]
        out = _demote_banner_headings(texts, ["text"], {"投标文件"})
        self.assertEqual(out[0].splitlines()[0], "# 第一章 总则")

    def test_non_text_pages_and_empty_repeating_are_untouched(self):
        texts = ["# 投标文件"]
        self.assertEqual(
            _demote_banner_headings(texts, ["scanned"], {"投标文件"}), texts
        )
        self.assertEqual(_demote_banner_headings(texts, ["text"], set()), texts)


class ShortIndexContinuationTest(unittest.TestCase):
    def test_index_entry_heading_is_demoted_not_frozen(self):
        texts = ["# 第一章 投标人须知 ……… 5\n正文"]
        out = _merge_short_index_continuations(texts, ["text"])
        # 目录项去掉标题标记，文本保留在正文流中。
        self.assertEqual(out[0].splitlines()[0], "第一章 投标人须知 ……… 5")
        self.assertIn("正文", out[0])

    def test_cross_page_entry_is_merged_and_demoted(self):
        texts = [
            "正文\n第三章 评标办法 ………",
            "……… 12\n正文继续",
        ]
        out = _merge_short_index_continuations(texts, ["text", "text"])
        # 上半部分暂存续写池，本页不输出。
        self.assertEqual(out[0], "正文")
        # 下页合并为完整条目，降级为普通文本。
        first = out[1].splitlines()[0]
        self.assertTrue(first.startswith("第三章 评标办法"))
        self.assertTrue(first.rstrip().endswith("12"))
        self.assertFalse(first.startswith("#"))

    def test_unmerged_carry_is_flushed_as_plain_text(self):
        texts = ["第三章 评标办法 ………", "# 第一章 总则\n正文"]
        out = _merge_short_index_continuations(texts, ["text", "text"])
        # 合并不成条目：carry 落定为普通行，首行保持原样。
        self.assertEqual(out[1].splitlines()[0], "第三章 评标办法 ………")
        self.assertEqual(out[1].splitlines()[1], "# 第一章 总则")

    def test_normal_headings_are_untouched(self):
        texts = ["# 第一章 总则\n正文内容"]
        out = _merge_short_index_continuations(texts, ["text"])
        self.assertEqual(out[0], texts[0])

    def test_non_text_page_breaks_continuation(self):
        texts = ["第三章 评标办法 ………", "图片页", "正文"]
        out = _merge_short_index_continuations(texts, ["text", "scanned", "text"])
        self.assertIn("第三章 评标办法 ………", out[0])
        self.assertEqual(out[1], "图片页")


class FinalChapterIdentityTest(unittest.TestCase):
    def test_long_and_short_toc_rows_never_repromote_after_demotion(self):
        entries = ["第一章 示例条件" + "." * 130 + "12", "第二章 示例表格 ……… 25", "1.1 范围......13"]
        text = "\n".join("# " + entry for entry in entries)
        cleaned = _merge_short_index_continuations([text], ["text"])
        promoted, fragments = _sectionize_pages(cleaned + ["第一章 示例条件\n本章正文。"])
        self.assertEqual(promoted[0].splitlines(), entries)
        self.assertEqual([fragment[1] for fragment in fragments[0]], [""])
        self.assertEqual(fragments[1][0][1], "第一章 示例条件")
        # Final sectionization must also reject untouched font-derived ATX.
        self.assertEqual(_sectionize_pages([text])[0][0].splitlines(), entries)

    def test_volume_chapters_and_subheadings_have_consistent_ownership(self):
        promoted, fragments = _sectionize_pages([
            "# 第一卷 示例商务\n## 第一章 示例公告\n公告正文。",
            "第二章 示例须知\n1. 总则\n正文。\n一、示例材料\n正文。",
            "# 第三章 示例评审\n正文。",
            "第二卷 示例技术\n第五章 示例规格\n正文。",
        ])
        paths = [fragment[1] for page in fragments for fragment in page]
        self.assertIn("第一卷 示例商务 > 第一章 示例公告", paths)
        self.assertIn("第一卷 示例商务 > 第二章 示例须知", paths)
        self.assertIn("第一卷 示例商务 > 第二章 示例须知 > 1. 总则", paths)
        self.assertIn("第一卷 示例商务 > 第三章 示例评审", paths)
        self.assertIn("第二卷 示例技术 > 第五章 示例规格", paths)
        self.assertTrue(promoted[1].startswith("## 第二章"))
        self.assertTrue(promoted[2].startswith("## 第三章"))

    def test_genuine_long_heading_or_numbered_body_is_not_removed(self):
        title = "第一章 " + "示例条件" * 30 + "2026"
        promoted, fragments = _sectionize_pages([title + "\n数值1.25保持原样。"])
        self.assertEqual(fragments[0][0][1], title)
        self.assertIn("数值1.25保持原样。", promoted[0])



if __name__ == "__main__":
    unittest.main()
