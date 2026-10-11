#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Route {
    #[default]
    Home,
    Search,
    Playlists,
    Albums,
    Songs,
    Album(String),
    Playlist(String),
}

impl Route {
    pub fn title(&self) -> &'static str {
        match self {
            Route::Home => "Home",
            Route::Search => "Search",
            Route::Playlists => "Playlists",
            Route::Albums => "Albums",
            Route::Songs => "Songs",
            Route::Album(_) => "Album",
            Route::Playlist(_) => "Playlist",
        }
    }

    #[cfg(feature = "dev")]
    pub fn parse(name: &str) -> Option<Route> {
        if let Some(id) = name.strip_prefix("playlist:") {
            return Some(Route::Playlist(id.to_owned()));
        }
        [
            Route::Home,
            Route::Search,
            Route::Playlists,
            Route::Albums,
            Route::Songs,
        ]
        .into_iter()
        .find(|r| r.title().eq_ignore_ascii_case(name))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Nav {
    Go(Route),
    Back,
    ToggleQueue,
    FocusSearch,
}

pub const BACK_LIMIT: usize = 32;

#[derive(Debug, Default)]
pub struct Shell {
    pub route: Route,
    back: Vec<Route>,
    pub queue_open: bool,
    pub focus_search: bool,
    pub query: String,
}

impl Shell {
    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn apply(&mut self, nav: Nav) {
        match nav {
            Nav::Go(route) if route != self.route => {
                if self.back.len() == BACK_LIMIT {
                    self.back.remove(0);
                }
                self.back.push(std::mem::replace(&mut self.route, route));
            }
            Nav::Go(_) => {}
            Nav::Back => {
                if let Some(route) = self.back.pop() {
                    self.route = route;
                }
            }
            Nav::ToggleQueue => self.queue_open = !self.queue_open,
            Nav::FocusSearch => {
                self.apply(Nav::Go(Route::Search));
                self.focus_search = true;
            }
        }
    }
}
