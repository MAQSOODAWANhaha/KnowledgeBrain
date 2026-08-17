.PHONY: proto run lint test clean

proto:
	@sh ./scripts/generate_proto.sh

run:
	@PYTHONPATH=.. python3 main.py

lint:
	@uvx ruff check .

test:
	@PYTHONPATH=.. uv run --with pytest pytest tests/

clean:
	@find . -name "*.pyc" -delete
	@find . -name "__pycache__" -delete
