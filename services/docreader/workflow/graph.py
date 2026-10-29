"""The single owner of the five tender analysis stages."""
from langgraph.graph import END, START, StateGraph

from docreader.tender.status import RunStatus
from docreader.workflow.state import STAGES, TenderState


def build_graph(nodes, checkpointer):
    builder = StateGraph(TenderState)
    for name in STAGES:
        builder.add_node(name, getattr(nodes, name))
    builder.add_edge(START, "register")
    builder.add_edge("register", "freeze")
    builder.add_edge("freeze", "extract")
    builder.add_edge("extract", "verify")
    builder.add_conditional_edges(
        "verify", lambda state: "blocked" if RunStatus(state["status"]) == RunStatus.BLOCKED else "confirm",
        {"blocked": END, "confirm": "confirm"},
    )
    builder.add_edge("confirm", END)
    return builder.compile(checkpointer=checkpointer)
