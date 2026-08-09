//! Blizzard's art, fetched once and kept small.
//!
//! Every picture in the application comes from `render.worldofwarcraft.com`.
//! That host is not an API host: it takes no token, answers no namespace and
//! does not spend the client's request quota, which is why a collection of
//! sixteen hundred mounts can be illustrated at all. `model::source::blizzard::media`
//! decides *which* URL; this fetches it.
//!
//! Three things stop that from being expensive.
//!
//! **Decoded small.** The service serves 600x600 renders and a grid draws them
//! at ninety-six. Decoding at full size and letting the GPU scale would be
//! about 1.4 MB of texture per mount; scaling during the decode makes it forty
//! kilobytes, so a few hundred can stay in memory instead of a few.
//!
//! **Kept on disk.** The bytes go under the cache directory, keyed by a hash of
//! the URL, so a second launch draws the same grid without touching the
//! network. [`Images::purge`] sweeps anything a month old, the same horizon the
//! store keeps for API responses.
//!
//! **Asked for once.** A URL already in flight collects further callers rather
//! than starting a second request, and one that has answered 403 — which is
//! what the service says for art that was never published — is not asked again
//! this session.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::gdk;
use gtk::gdk_pixbuf::Pixbuf;
use gtk::gio;
use gtk::glib;

use super::http::Http;
use crate::model::source::{Outcome, Request, SourceId};

/// How many decoded textures to keep.
///
/// A grid recycles about forty cells and a person scrolls back as often as
/// forward, so this is sized to hold several screens either side of wherever
/// they are. At ninety-six pixels that is a handful of megabytes.
const CACHE_SIZE: usize = 512;

/// How long a cached image file lives.
///
/// The same thirty days the store keeps API responses for. The art is not an
/// API response and is not covered by that term, but a cache directory that
/// grows forever is its own problem and one horizon is easier to reason about
/// than two.
const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// What a caller gets back.
type Deliver = Box<dyn FnOnce(&gdk::Texture)>;

#[derive(Clone)]
pub struct Images {
    inner: Rc<Inner>,
}

struct Inner {
    http: Http,
    directory: PathBuf,
    cache: RefCell<Lru>,
    /// URLs with a request out, and everyone waiting on it.
    pending: RefCell<HashMap<Key, Vec<Deliver>>>,
    /// URLs the service has refused. Art that was never published answers 403
    /// forever, and asking again every time a cell scrolls past would be a
    /// request per frame.
    refused: RefCell<HashSet<String>>,
}

/// A URL at a size. The same render is wanted small in a grid and large in a
/// dialog, and those are different textures.
type Key = (String, i32);

impl Default for Images {
    fn default() -> Self {
        Images::new()
    }
}

impl Images {
    pub fn new() -> Self {
        let directory = glib::user_cache_dir().join("armory").join("media");
        let _ = std::fs::create_dir_all(&directory);

        Images {
            inner: Rc::new(Inner {
                http: Http::new(),
                directory,
                cache: RefCell::new(Lru::new(CACHE_SIZE)),
                pending: RefCell::new(HashMap::new()),
                refused: RefCell::new(HashSet::new()),
            }),
        }
    }

    /// Where the bytes for one URL live.
    fn path(&self, url: &str) -> PathBuf {
        // A URL is not a filename — it carries slashes, query strings and, for
        // a character portrait, a hash longer than some filesystems allow.
        let digest = glib::compute_checksum_for_string(glib::ChecksumType::Sha256, url)
            .map(|digest| digest.to_string())
            .unwrap_or_else(|| url.len().to_string());
        self.inner.directory.join(format!("{digest}.img"))
    }

    /// Hand over a texture for `url`, now or when it arrives.
    ///
    /// `deliver` is called at most once and never with a failure: a picture
    /// that cannot be had leaves whatever the caller was showing in its place,
    /// which for every caller here is a placeholder that already reads
    /// correctly.
    pub fn load<F: FnOnce(&gdk::Texture) + 'static>(&self, url: &str, size: i32, deliver: F) {
        let key = (url.to_string(), size);

        if let Some(texture) = self.inner.cache.borrow_mut().get(&key) {
            deliver(&texture);
            return;
        }
        if self.inner.refused.borrow().contains(url) {
            return;
        }

        // Already asked for. Join the queue rather than sending a second
        // request for the same bytes — a grid binding forty cells against the
        // same placeholder would otherwise send forty.
        if let Some(waiting) = self.inner.pending.borrow_mut().get_mut(&key) {
            waiting.push(Box::new(deliver));
            return;
        }
        self.inner
            .pending
            .borrow_mut()
            .insert(key.clone(), vec![Box::new(deliver)]);

        // On disk from a previous launch. Decoding is a few milliseconds and
        // happens here rather than on a worker, because the alternative is
        // handing textures between threads for something that costs less than
        // a frame.
        if let Ok(bytes) = std::fs::read(self.path(url)) {
            self.finish(&key, &bytes, false);
            return;
        }

        let images = self.clone();
        let key_for_reply = key.clone();
        self.inner.http.fetch(
            Request::get(SourceId::BlizzardGameData, url),
            move |outcome| match outcome {
                Outcome::Found(response) => images.finish(&key_for_reply, &response.body, true),
                // Anything else is art that is not there. The render service
                // answers 403 for a texture that was never published, which
                // `http` reads as a privacy refusal — true of the API, not of a
                // CDN, and either way the answer is the same: no picture.
                _ => {
                    images
                        .inner
                        .refused
                        .borrow_mut()
                        .insert(key_for_reply.0.clone());
                    images.inner.pending.borrow_mut().remove(&key_for_reply);
                }
            },
        );
    }

    /// Decode, cache, and answer everyone waiting.
    fn finish(&self, key: &Key, bytes: &[u8], write: bool) {
        let waiting = self.inner.pending.borrow_mut().remove(key);

        let Some(texture) = decode(bytes, key.1) else {
            // Bytes that will not decode are not worth keeping or retrying. A
            // truncated file from an interrupted launch lands here, and
            // deleting it is what lets the next launch fetch it properly.
            let _ = std::fs::remove_file(self.path(&key.0));
            self.inner.refused.borrow_mut().insert(key.0.clone());
            return;
        };

        if write {
            let _ = std::fs::write(self.path(&key.0), bytes);
        }
        self.inner.cache.borrow_mut().put(key.clone(), &texture);

        for deliver in waiting.into_iter().flatten() {
            deliver(&texture);
        }
    }

    /// Drop cached art nobody has looked at in a month.
    ///
    /// Returns how many files went. Called at shutdown alongside the store's
    /// own sweep, so a first paint never waits on a directory walk.
    pub fn purge(&self) -> usize {
        let Ok(entries) = std::fs::read_dir(&self.inner.directory) else {
            return 0;
        };
        let now = SystemTime::now();
        let mut removed = 0;

        for entry in entries.flatten() {
            let stale = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .map(|at| now.duration_since(at).unwrap_or_default() > MAX_AGE)
                .unwrap_or(false);
            if stale && std::fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
        removed
    }
}

/// Decode to a texture no larger than `size` on its longest side.
///
/// The scaling happens inside the decode rather than after it, which is the
/// whole saving: a 600x600 render never exists at full size in memory, and what
/// reaches the texture is already the size it will be drawn at.
fn decode(bytes: &[u8], size: i32) -> Option<gdk::Texture> {
    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from(bytes));
    let pixbuf =
        Pixbuf::from_stream_at_scale(&stream, size, size, true, gio::Cancellable::NONE).ok()?;

    // Straight from the pixbuf's own buffer. `Texture::for_pixbuf` did this and
    // was deprecated in GTK 4.20; the rowstride matters, because a pixbuf's
    // rows are padded and a texture built as though they were not comes out
    // sheared.
    Some(
        gdk::MemoryTexture::new(
            pixbuf.width(),
            pixbuf.height(),
            if pixbuf.has_alpha() {
                gdk::MemoryFormat::R8g8b8a8
            } else {
                gdk::MemoryFormat::R8g8b8
            },
            &pixbuf.read_pixel_bytes(),
            pixbuf.rowstride() as usize,
        )
        .upcast(),
    )
}

/// A least-recently-used map, sized in entries.
struct Lru {
    limit: usize,
    entries: HashMap<Key, gdk::Texture>,
    order: VecDeque<Key>,
}

impl Lru {
    fn new(limit: usize) -> Self {
        Lru {
            limit,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&mut self, key: &Key) -> Option<gdk::Texture> {
        let texture = self.entries.get(key)?.clone();
        if let Some(at) = self.order.iter().position(|held| held == key) {
            self.order.remove(at);
        }
        self.order.push_back(key.clone());
        Some(texture)
    }

    fn put(&mut self, key: Key, texture: &gdk::Texture) {
        if self.entries.insert(key.clone(), texture.clone()).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > self.limit {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }
}

// -- the widget --------------------------------------------------------------

/// A square of art, with something sensible in it until the art lands.
///
/// A plain `GtkPicture` handed to an image loader is a bug waiting for a fast
/// scroll: a list view recycles cells, so the callback for the row that has
/// scrolled off arrives after the same widget has been rebound to a different
/// one, and the wrong mount appears. This holds the URL it currently wants and
/// ignores anything else, which is the whole reason it is a widget rather than
/// a function.
mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Art {
        pub picture: RefCell<Option<gtk::Picture>>,
        pub fallback: RefCell<Option<gtk::Image>>,
        /// What this widget is currently showing or waiting for. A delivery
        /// that does not match is a delivery for whoever used to be here.
        pub wanted: RefCell<Option<String>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Art {
        const NAME: &'static str = "ArmoryArt";
        type Type = super::Art;
        type ParentType = adw::Bin;
    }

    impl ObjectImpl for Art {}
    impl WidgetImpl for Art {}
    impl BinImpl for Art {}
}

glib::wrapper! {
    pub struct Art(ObjectSubclass<imp::Art>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Art {
    /// A square of art `size` pixels on a side, showing `placeholder` until
    /// there is something better.
    pub fn new(size: i32, placeholder: &str) -> Self {
        let art: Self = glib::Object::builder().build();
        art.set_size_request(size, size);
        art.set_overflow(gtk::Overflow::Hidden);
        art.add_css_class("art");

        let fallback = gtk::Image::builder()
            .icon_name(placeholder)
            .pixel_size((size / 2).max(16))
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        fallback.add_css_class("dimmed");

        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .can_shrink(true)
            .visible(false)
            .build();

        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&fallback));
        overlay.add_overlay(&picture);

        *art.imp().picture.borrow_mut() = Some(picture);
        *art.imp().fallback.borrow_mut() = Some(fallback);
        art.set_child(Some(&overlay));
        art
    }

    /// Show the art at `url`, or keep the placeholder if there is none.
    ///
    /// Safe to call on a recycled widget: the previous request is disowned
    /// rather than cancelled, so its bytes still reach the cache and the next
    /// row that wants them gets them for nothing.
    pub fn show(&self, images: &Images, url: Option<&str>, size: i32) {
        let imp = self.imp();
        *imp.wanted.borrow_mut() = url.map(str::to_string);

        let Some(picture) = imp.picture.borrow().clone() else {
            return;
        };
        picture.set_visible(false);
        picture.set_paintable(gdk::Paintable::NONE);

        let Some(url) = url else { return };
        let art = self.clone();
        let expecting = url.to_string();

        images.load(url, size, move |texture| {
            // The cell may have been rebound while this was in flight.
            if art.imp().wanted.borrow().as_deref() != Some(expecting.as_str()) {
                return;
            }
            if let Some(picture) = art.imp().picture.borrow().as_ref() {
                picture.set_paintable(Some(texture));
                picture.set_visible(true);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cache_forgets_the_least_recently_looked_at() {
        // Sized in entries rather than bytes, so the eviction order is the
        // whole of the policy and worth pinning down.
        let mut lru = Lru::new(2);
        let key = |name: &str| (name.to_string(), 96);

        // No display is needed to exercise the ordering, so the textures are
        // stand-ins built from a single pixel.
        let texture = || {
            gdk::MemoryTexture::new(
                1,
                1,
                gdk::MemoryFormat::R8g8b8a8,
                &glib::Bytes::from(&[0u8, 0, 0, 255][..]),
                4,
            )
            .upcast::<gdk::Texture>()
        };

        lru.put(key("a"), &texture());
        lru.put(key("b"), &texture());
        // Touching "a" makes "b" the oldest.
        assert!(lru.get(&key("a")).is_some());
        lru.put(key("c"), &texture());

        assert!(lru.get(&key("b")).is_none(), "the untouched one goes");
        assert!(lru.get(&key("a")).is_some());
        assert!(lru.get(&key("c")).is_some());
    }

    #[test]
    fn one_url_at_two_sizes_is_two_textures() {
        // A grid wants ninety-six pixels and a dialog wants three hundred.
        // Keying on the URL alone would hand the dialog the thumbnail.
        let mut lru = Lru::new(4);
        let texture = gdk::MemoryTexture::new(
            1,
            1,
            gdk::MemoryFormat::R8g8b8a8,
            &glib::Bytes::from(&[0u8, 0, 0, 255][..]),
            4,
        )
        .upcast::<gdk::Texture>();

        lru.put(("https://render/x.jpg".into(), 96), &texture);
        assert!(lru.get(&("https://render/x.jpg".into(), 96)).is_some());
        assert!(lru.get(&("https://render/x.jpg".into(), 320)).is_none());
    }

    #[test]
    fn bytes_that_are_not_an_image_decode_to_nothing_rather_than_panicking() {
        // Blizzard's edge answers HTML during maintenance, with a 200.
        assert!(decode(b"<html>maintenance</html>", 96).is_none());
        assert!(decode(&[], 96).is_none());
    }
}
