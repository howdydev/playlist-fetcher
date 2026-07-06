pkgname=playlist-fetcher-git
pkgver=0.1.0.r0.g1124a42
pkgrel=1
pkgdesc="Desktop GUI queue manager for spotdl/scdl downloads"
arch=('x86_64')
url="https://github.com/howdydev/playlist-fetcher"
license=('MIT')
depends=('gcc-libs' 'gtk3' 'webkit2gtk-4.1')
makedepends=('rust' 'cargo' 'git')
optdepends=(
    'spotdl: Spotify downloads'
    'scdl: SoundCloud downloads'
    'ffmpeg: required by spotdl'
)
provides=('playlist-fetcher')
conflicts=('playlist-fetcher')
source=("$pkgname::git+ssh://git@github.com/howdydev/playlist-fetcher.git")
sha256sums=('SKIP')

pkgver() {
    cd "$pkgname"
    git describe --long --tags | sed 's/^v//;s/\([^-]*-g\)/r\1/;s/-/./g'
}

build() {
    cd "$pkgname"
    cargo build --release --locked
}

package() {
    cd "$pkgname"
    install -Dm755 "target/release/playlist-fetcher" "$pkgdir/usr/bin/playlist-fetcher"
}
