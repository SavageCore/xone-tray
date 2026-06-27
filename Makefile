.PHONY: build release test lint fmt clean install-udev rpm deb

build:
	cargo build

release:
	cargo build --release

test:
	cargo test

lint:
	cargo fmt --check
	cargo clippy -- -D warnings

fmt:
	cargo fmt

clean:
	cargo clean

install-udev:
	sudo cp packaging/50-xone-tray.rules /etc/udev/rules.d/
	sudo udevadm control --reload-rules && sudo udevadm trigger

rpm: release
	cargo generate-rpm

deb: release
	cargo deb
