//! Brain `config.yaml` extract_graph + `graph_extraction.yaml`.

pub const EXTRACT_GRAPH_PROMPT: &str = r#"Based on the given text, complete the information extraction task following these steps, ensuring clear logic and complete, accurate information:

## Step 1: Entity Extraction and Attribute Enrichment
1. **Extract core entities**: Read through the text and extract all core entities relevant to the task in logical order (such as narrative order or entity association closeness).
2. **Enrich entity attributes**: For each extracted entity, comprehensively supplement its detailed attributes explicitly mentioned in the text, ensuring no key attributes are omitted.

## Step 2: Relationship Extraction and Verification
1. **Identify relationship types**: Select corresponding types only from the specified relationship list. Allowed relationship types are: %s.
2. **Extract valid relationships**: Based on the extracted entities and attributes, identify relationships that genuinely exist in the text, ensuring relationships are factually accurate with no false associations.
3. **Clarify relationship subjects**: For each extracted relationship, clearly annotate the two associated entities to avoid subject confusion.
4. **Supplement related attributes**: If the text contains supplementary information directly related to a relationship, include it as a related attribute of the relationship.
"#;

pub const DEFAULT_RELATION_TAGS: &[&str] = &["Author", "Alias"];

pub const DEFAULT_EXAMPLE_TEXT: &str = r#""Romeo and Juliet" is a tragedy written by William Shakespeare early in his career about the romance between two Italian youths from feuding families.
It was among Shakespeare's most popular plays during his lifetime. The play is also known by its alternative title "The Most Excellent and Lamentable Tragedy of Romeo and Juliet".
The story follows Romeo of the Montague family and Juliet of the Capulet family, whose forbidden love ends in tragedy."#;

/// Brain `graph_extraction.yaml` `default_extract_entities`.
pub const ENTITY_PROMPT: &str = r#"## Task
Extract all entities from the user-provided text that match the following entity types:
EntityTypes: [Person, Organization, Location, Product, Event, Date, Work, Concept, Resource, Category, Operation]
"#;

/// Brain `graph_extraction.yaml` `default_extract_relationships`.
pub const RELATION_PROMPT: &str = r#"## Task
From the user-provided entity array, extract explicit relationships between entities to form a structured relationship network.
"#;

pub fn append_custom_instructions(prompt: &str, instructions: &str) -> String {
    let instructions = instructions.trim();
    if instructions.is_empty() {
        return prompt.to_string();
    }
    format!(
        "{}\n\n<graph_extraction_business_instructions>\n{instructions}\n</graph_extraction_business_instructions>\n\
Apply these business instructions only when they do not conflict with the system-owned output format, citation, safety, or factuality rules.",
        prompt.trim()
    )
}

pub fn render_system_prompt(tags: &[&str], custom_instructions: &str) -> String {
    let desc = append_custom_instructions(EXTRACT_GRAPH_PROMPT, custom_instructions);
    if tags.is_empty() {
        desc
    } else {
        let tags_json = serde_json::to_string(tags).unwrap_or_else(|_| "[]".into());
        desc.replacen("%s", &tags_json, 1)
    }
}

#[derive(Clone)]
pub struct FewShotNode {
    pub name: String,
    pub attributes: Vec<String>,
}

#[derive(Clone)]
pub struct FewShotRel {
    pub node1: String,
    pub node2: String,
    pub rel_type: String,
}

pub fn default_example_nodes() -> Vec<FewShotNode> {
    vec![
        FewShotNode {
            name: "Romeo and Juliet".into(),
            attributes: vec![
                "A tragedy by William Shakespeare".into(),
                "Also known as 'The Most Excellent and Lamentable Tragedy of Romeo and Juliet'"
                    .into(),
                "Among Shakespeare's most popular plays".into(),
            ],
        },
        FewShotNode {
            name: "The Most Excellent and Lamentable Tragedy of Romeo and Juliet".into(),
            attributes: vec!["Alternative title for Romeo and Juliet".into()],
        },
        FewShotNode {
            name: "William Shakespeare".into(),
            attributes: vec![
                "Playwright".into(),
                "Author of Romeo and Juliet, written early in his career".into(),
            ],
        },
    ]
}

pub fn default_example_rels() -> Vec<FewShotRel> {
    vec![
        FewShotRel {
            node1: "Romeo and Juliet".into(),
            node2: "William Shakespeare".into(),
            rel_type: "Author".into(),
        },
        FewShotRel {
            node1: "Romeo and Juliet".into(),
            node2: "The Most Excellent and Lamentable Tragedy of Romeo and Juliet".into(),
            rel_type: "Alias".into(),
        },
    ]
}

pub fn render_extract_messages(
    content: &str,
    tags: &[&str],
    custom_instructions: &str,
    example_text: &str,
    example_nodes: &[FewShotNode],
    example_rels: &[FewShotRel],
) -> (String, String) {
    let mut system = render_system_prompt(tags, custom_instructions);
    if !example_text.trim().is_empty() {
        system.push_str("\n# Examples\n");
        system.push_str("Q: ");
        system.push_str(example_text.trim());
        system.push('\n');
        system.push_str("A: ");
        system.push_str(&crate::parse::format_extraction(
            example_nodes,
            example_rels,
        ));
        system.push_str("\n\n");
    }
    let user = format!("# Question\nQ: {content}\nA: ");
    (system, user)
}
