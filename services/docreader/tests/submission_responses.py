"""Explicit source-to-assembly responses shared by authoring test executors."""


def provided_template_ids(payload):
    return [part["part_id"] for part in payload.get("template_parts", [])]


def assemble_independent_candidates(payload):
    """Retain every fixture candidate, with its explicit parts and hierarchy."""
    chapters = []
    for candidate in payload["chapters"]:
        siblings = chapters
        path = candidate.get("title_path", [candidate["title"]])
        for title in path:
            chapter = next((entry for entry in siblings if entry["title"] == title), None)
            if chapter is None:
                chapter = {"title": title, "is_volume": False, "targets": [], "children": []}
                siblings.append(chapter)
            siblings = chapter["children"]
        chapter["is_volume"] = candidate.get("is_volume", False)
        chapter["targets"].extend({
            "kind": target["kind"], "purpose": target["purpose"],
            "target_ids": [target["target_id"]],
            "template_part_ids": list(target.get("template_part_ids", [])),
        } for target in candidate["targets"])
    return {"chapters": chapters}
