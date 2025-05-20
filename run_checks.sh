failed=0

echo "Running format check"
cargo fmt --check

if [ "$(cargo fmt --check | wc -l)" -gt 0 ]; then
    echo "Formatter Check Failed."
    failed=1
fi

echo "Running go test"

cargo test

if [ $? -ne 0 ]; then
    echo "Go Test Failed."
    failed=1
fi

if [ "$failed" -eq 1 ]; then
    echo "Failed!"
    exit 1
else
    echo "Success!"
    exit 0
fi
