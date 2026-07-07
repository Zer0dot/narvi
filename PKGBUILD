# Maintainer: zer0dot <zer0dot.dev@gmail.com>
pkgname=narvi
pkgver=0.1.0
pkgrel=1
pkgdesc="Real-time color-management suite for Hyprland (vibrance, temperature, gamma, RGB)"
arch=('x86_64')
url="https://github.com/zer0dot/narvi"
license=('MIT')
depends=('hyprland' 'vulkan-icd-loader' 'wayland' 'libxkbcommon')
makedepends=('cargo')
source=("$pkgname-$pkgver.tar.gz::$url/archive/v$pkgver.tar.gz")
sha256sums=('SKIP')

build() {
  cd "$pkgname-$pkgver"
  export RUSTUP_TOOLCHAIN=stable
  cargo build --release --locked
}

check() {
  cd "$pkgname-$pkgver"
  cargo test --release --locked
}

package() {
  cd "$pkgname-$pkgver"
  for bin in narvi narvid narvi-gui narvi-tray; do
    install -Dm755 "target/release/$bin" "$pkgdir/usr/bin/$bin"
  done
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
