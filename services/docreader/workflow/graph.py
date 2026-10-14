"""The single owner of the six tender business stages."""
from langgraph.graph import END, START, StateGraph

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
        "verify", lambda state: "blocked" if state.get("status") == "blocked" else "confirm",
        {"blocked": END, "confirm": "confirm"},
    )
    builder.add_edge("confirm", "plan")
    builder.add_edge("plan", END)
    return builder.compile(checkpointer=checkpointer)
