PKG      := fmg
VERSION  := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
ARCH     := $(shell dpkg --print-architecture)
DEB_NAME := $(PKG)_$(VERSION)_$(ARCH)
DEB_DIR  := target/deb/$(DEB_NAME)

.PHONY: build clean deb

build:
	cargo build --release

deb: build
	rm -rf $(DEB_DIR)
	mkdir -p $(DEB_DIR)/DEBIAN
	mkdir -p $(DEB_DIR)/usr/bin
	cp target/release/$(PKG) $(DEB_DIR)/usr/bin/
	{ echo "Package: $(PKG)"; \
	  echo "Version: $(VERSION)"; \
	  echo "Section: utils"; \
	  echo "Priority: optional"; \
	  echo "Architecture: $(ARCH)"; \
	  echo "Maintainer: $(PKG) contributors"; \
	  echo "Description: Fast, Obsidian-native CLI for traversing [[WikiLink]] relationships in frontmatter markdown vaults"; \
	} > $(DEB_DIR)/DEBIAN/control
	dpkg-deb --root-owner-group --build $(DEB_DIR) target/deb/$(DEB_NAME).deb
	@echo "\n  Built: target/deb/$(DEB_NAME).deb\n"

clean:
	cargo clean
	rm -rf target/deb
