/// Brain `expectedSubtasks` (knowledge_post_process.go).
pub fn expected_subtasks(
    text_count: usize,
    ocr_caption_count: usize,
    question_enabled: bool,
    needs_embedding: bool,
    wiki_enabled: bool,
    graph_enabled: bool,
    clone_keep: bool,
) -> usize {
    let text_like = text_count + ocr_caption_count;
    let will_summary = text_like > 0 && !clone_keep;
    let will_question = will_summary && needs_embedding && question_enabled;
    let will_wiki = wiki_enabled && text_like > 0;
    let question_chunks = if will_question { text_count } else { 0 };
    let question_batches = question_chunks.div_ceil(20);
    let graph_count = if graph_enabled { text_like } else { 0 };
    let summary = usize::from(will_summary);
    let wiki = usize::from(will_wiki);
    summary + question_batches + wiki + graph_count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_table_cases() {
        // text, ocr, q, embed, wiki, graph → N
        assert_eq!(expected_subtasks(0, 0, true, true, true, true, false), 0);
        assert_eq!(expected_subtasks(25, 0, true, true, true, true, false), 29);
        assert_eq!(expected_subtasks(25, 3, true, true, true, true, false), 32);
        assert_eq!(expected_subtasks(10, 0, false, true, true, false, false), 2);
        assert_eq!(
            expected_subtasks(10, 0, true, false, false, false, false),
            1
        );
    }

    #[test]
    fn clone_keep_only_wiki_graph() {
        assert_eq!(
            expected_subtasks(10, 2, true, true, true, true, true),
            1 + 12
        );
        assert_eq!(expected_subtasks(10, 0, true, true, false, false, true), 0);
    }
}
