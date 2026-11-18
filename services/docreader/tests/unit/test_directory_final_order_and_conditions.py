from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.directory_compile import apply_parameters, compile_plans
from test_directory_parameter_continuations import source_case, parameter, response
from test_directory_condition_merge import candidate, DOCUMENTS
from test_directory_compile import window


def test_host_orders_complete_parameters_by_registered_source_position():
    documents, compiled = source_case()
    value = response(parameter(title='字符处理与替代方案'))
    value["chapters"][0]["items"].insert(0, {
        "document_id": "tender", "page": 1, "start_line": 3, "end_line": 3,
        "content_start_line": 3, "continuation_windows": [], 'title': '独立技术要求'})
    accepted = pipeline._accept_parameter_plan(value, compiled, documents, "technical")
    apply_parameters(compiled, documents, accepted["chapters"])
    assert [chapter["title"] for chapter in compiled["chapters"][1:]] == [
        "★支持数字、字", "下一独立要求。"]
    assert "完整替代方案" in compiled["chapters"][1]["text"]


def test_accepted_second_plan_condition_has_a_named_manual_decision():
    first, second = candidate(condition=2), candidate(condition=3, extra=True)
    second["nodes"][1]["parent_id"] = "form"
    second["scopes"][0]["items"][1]["condition"] = window(0, 2)
    compiled = compile_plans([first, second], DOCUMENTS)
    assert any(chapter["title"] == "其他原式" and chapter["condition"] == "适用于代理商"
               for chapter in compiled["chapters"])
    assert any(issue["code"] == "outline_directory_applicability_unconfirmed"
               and "其他原式" in issue["detail"] and "适用于代理商" in issue["detail"]
               for issue in compiled["issues"])
