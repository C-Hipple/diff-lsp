check:
	@failed=0; \
	echo "Compiling"; \
	cargo build; \
	if [ $$? -ne 0 ]; then \
		echo "Formatter Check Failed."; \
		failed=1; \
	fi; \
	echo "Running format check"; \
	cargo fmt --check; \
	if [ $$? -ne 0 ]; then \
		echo "Formatter Check Failed."; \
		failed=1; \
	fi; \
	echo "Running cargo test"; \
	cargo test; \
	if [ $$? -ne 0 ]; then \
		echo "Cargo Test Failed."; \
		failed=1; \
	fi; \
	if [ "$$failed" -eq 1 ]; then \
		echo "Failed!"; \
		exit 1; \
	else \
		echo "Success!"; \
		exit 0; \
	fi
