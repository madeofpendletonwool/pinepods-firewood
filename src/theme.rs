use anyhow::Result;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeResponse {
    pub theme: String,
}

#[derive(Debug, Clone)]
pub struct ThemeColors {
    pub background: Color,
    pub primary: Color,
    pub secondary: Color,
    pub text: Color,
    pub text_secondary: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub border: Color,
    pub highlight: Color,
    pub container: Color,
}

impl Default for ThemeColors {
    fn default() -> Self {
        Self::light()
    }
}

impl ThemeColors {
    pub fn light() -> Self {
        Self {
            background: Color::Rgb(249, 249, 249),      // #f9f9f9
            primary: Color::Rgb(0, 153, 225),           // #0099e1
            secondary: Color::Rgb(241, 241, 241),       // #f1f1f1
            text: Color::Rgb(74, 74, 74),               // #4a4a4a
            text_secondary: Color::Rgb(150, 151, 151),  // #969797
            accent: Color::Rgb(112, 86, 151),           // #705697
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            border: Color::Rgb(74, 74, 74),             // #4a4a4a
            highlight: Color::Rgb(0, 153, 225),         // #0099e1
            container: Color::Rgb(232, 232, 232),       // #e8e8e8
        }
    }

    pub fn nordic() -> Self {
        Self {
            background: Color::Rgb(60, 66, 82),         // #3C4252
            primary: Color::Rgb(53, 80, 175),           // #3550af
            secondary: Color::Rgb(46, 52, 64),          // #2e3440
            text: Color::Rgb(246, 245, 244),            // #f6f5f4
            text_secondary: Color::Rgb(109, 116, 127),  // #6d747f
            accent: Color::Rgb(93, 128, 170),           // #5d80aa
            success: Color::Rgb(163, 190, 140),         // #a3be8c
            warning: Color::Rgb(235, 203, 139),         // #ebcb8b
            error: Color::Rgb(191, 97, 106),            // #bf616a
            border: Color::Black,
            highlight: Color::Rgb(93, 128, 170),        // #5d80aa
            container: Color::Rgb(43, 47, 58),          // #2b2f3a
        }
    }

    pub fn abyss() -> Self {
        Self {
            background: Color::Rgb(0, 12, 24),          // #000C18
            primary: Color::Rgb(50, 111, 239),          // #326fef
            secondary: Color::Rgb(5, 19, 54),           // #051336
            text: Color::Rgb(246, 245, 244),            // #f6f5f4
            text_secondary: Color::Rgb(131, 131, 133),  // #838385
            accent: Color::Rgb(200, 170, 125),          // #c8aa7d
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            border: Color::Black,
            highlight: Color::Rgb(21, 32, 55),          // #152037
            container: Color::Rgb(6, 25, 64),           // #061940
        }
    }

    pub fn soft_lavender() -> Self {
        Self {
            background: Color::Rgb(245, 240, 255),         // #f5f0ff
            primary: Color::Rgb(147, 113, 217),            // #9371d9
            secondary: Color::Rgb(240, 234, 248),          // #f0eaf8
            text: Color::Rgb(80, 69, 110),                 // #50456e
            text_secondary: Color::Rgb(124, 106, 153),     // #7c6a99
            accent: Color::Rgb(201, 188, 238),             // #c9bcee
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Rgb(226, 98, 149),               // #e26295
            border: Color::Rgb(189, 180, 209),             // #bdb4d1
            highlight: Color::Rgb(164, 126, 233),          // #a47ee9
            container: Color::Rgb(232, 225, 247),          // #e8e1f7
        }
    }

    pub fn minty_fresh() -> Self {
        Self {
            background: Color::Rgb(241, 249, 246),         // #f1f9f6
            primary: Color::Rgb(61, 157, 130),             // #3d9d82
            secondary: Color::Rgb(231, 246, 241),          // #e7f6f1
            text: Color::Rgb(45, 110, 91),                 // #2d6e5b
            text_secondary: Color::Rgb(91, 161, 146),      // #5ba192
            accent: Color::Rgb(133, 194, 176),             // #85c2b0
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Rgb(231, 118, 112),              // #e77670
            border: Color::Rgb(176, 217, 203),             // #b0d9cb
            highlight: Color::Rgb(77, 171, 146),           // #4dab92
            container: Color::Rgb(221, 240, 232),          // #ddf0e8
        }
    }

    pub fn warm_vanilla() -> Self {
        Self {
            background: Color::Rgb(253, 246, 233),         // #fdf6e9
            primary: Color::Rgb(198, 160, 109),            // #c6a06d
            secondary: Color::Rgb(248, 238, 222),          // #f8eede
            text: Color::Rgb(109, 73, 34),                 // #6d4922
            text_secondary: Color::Rgb(160, 128, 82),      // #a08052
            accent: Color::Rgb(230, 209, 172),             // #e6d1ac
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Rgb(217, 104, 76),               // #d9684c
            border: Color::Rgb(216, 199, 167),             // #d8c7a7
            highlight: Color::Rgb(217, 177, 101),          // #d9b165
            container: Color::Rgb(245, 231, 209),          // #f5e7d1
        }
    }

    pub fn coastal_blue() -> Self {
        Self {
            background: Color::Rgb(240, 245, 250),         // #f0f5fa
            primary: Color::Rgb(76, 135, 197),             // #4c87c5
            secondary: Color::Rgb(232, 240, 248),          // #e8f0f8
            text: Color::Rgb(44, 93, 143),                 // #2c5d8f
            text_secondary: Color::Rgb(92, 137, 183),      // #5c89b7
            accent: Color::Rgb(140, 176, 209),             // #8cb0d1
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Rgb(232, 111, 111),              // #e86f6f
            border: Color::Rgb(176, 205, 227),             // #b0cde3
            highlight: Color::Rgb(89, 146, 202),           // #5992ca
            container: Color::Rgb(222, 232, 243),          // #dee8f3
        }
    }

    pub fn paper_cream() -> Self {
        Self {
            background: Color::Rgb(250, 247, 242),         // #faf7f2
            primary: Color::Rgb(161, 151, 136),            // #a19788
            secondary: Color::Rgb(245, 242, 236),          // #f5f2ec
            text: Color::Rgb(74, 68, 57),                  // #4a4439
            text_secondary: Color::Rgb(132, 127, 116),     // #847f74
            accent: Color::Rgb(216, 208, 192),             // #d8d0c0
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Rgb(209, 108, 98),               // #d16c62
            border: Color::Rgb(211, 206, 195),             // #d3cec3
            highlight: Color::Rgb(179, 168, 148),          // #b3a894
            container: Color::Rgb(238, 233, 224),          // #eee9e0
        }
    }

    pub fn dark() -> Self {
        Self {
            background: Color::Rgb(42, 43, 51),            // #2a2b33
            primary: Color::Rgb(74, 83, 94),               // #4a535e
            secondary: Color::Rgb(50, 51, 59),             // #32333b
            text: Color::Rgb(246, 245, 244),               // #f6f5f4
            text_secondary: Color::Rgb(246, 245, 244),     // #f6f5f4
            accent: Color::Rgb(74, 83, 94),                // #4a535e
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            border: Color::Black,
            highlight: Color::Rgb(75, 85, 99),             // #4b5563
            container: Color::Rgb(27, 29, 30),             // #1b1d1e
        }
    }

    pub fn nordic_light() -> Self {
        Self {
            background: Color::Rgb(236, 239, 244),         // #eceff4
            primary: Color::Rgb(41, 133, 207),             // #2984ce
            secondary: Color::Rgb(229, 233, 240),          // #e5e9f0
            text: Color::Rgb(101, 109, 118),               // #656d76
            text_secondary: Color::Rgb(154, 162, 170),     // #9aa2aa
            accent: Color::Rgb(135, 141, 149),             // #878d95
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            border: Color::Black,
            highlight: Color::Rgb(42, 133, 207),           // #2a85cf
            container: Color::Rgb(216, 222, 233),          // #d8dee9
        }
    }

    pub fn dracula() -> Self {
        Self {
            background: Color::Rgb(40, 42, 54),            // #282A36
            primary: Color::Rgb(189, 147, 249),            // #bd93f9
            secondary: Color::Rgb(38, 38, 38),             // #262626
            text: Color::Rgb(246, 245, 244),               // #f6f5f4
            text_secondary: Color::Rgb(246, 245, 244),     // #f6f5f4
            accent: Color::Rgb(114, 117, 128),             // #727580
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            border: Color::Black,
            highlight: Color::Rgb(75, 85, 99),             // #4b5563
            container: Color::Rgb(25, 26, 33),             // #191a21
        }
    }

    pub fn catppuccin_mocha_mauve() -> Self {
        Self {
            background: Color::Rgb(30, 30, 46),            // #1e1e2e
            primary: Color::Rgb(166, 227, 161),            // #a6e3a1
            secondary: Color::Rgb(17, 17, 27),             // #11111b
            text: Color::Rgb(205, 214, 244),               // #cdd6f4
            text_secondary: Color::Rgb(186, 194, 222),     // #bac2de
            accent: Color::Rgb(203, 166, 247),             // #cba6f7
            success: Color::Rgb(166, 227, 161),            // #a6e3a1
            warning: Color::Yellow,
            error: Color::Rgb(243, 139, 168),              // #f38ba8
            border: Color::Rgb(203, 166, 247),             // #cba6f7
            highlight: Color::Rgb(108, 112, 134),          // #6c7086
            container: Color::Rgb(49, 50, 68),             // #313244
        }
    }

    pub fn midnight_ocean() -> Self {
        Self {
            background: Color::Rgb(15, 23, 42),            // #0f172a
            primary: Color::Rgb(14, 165, 233),             // #0ea5e9
            secondary: Color::Rgb(30, 41, 59),             // #1e293b
            text: Color::Rgb(226, 232, 240),               // #e2e8f0
            text_secondary: Color::Rgb(148, 163, 184),     // #94a3b8
            accent: Color::Rgb(56, 189, 248),              // #38bdf8
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Rgb(239, 68, 68),                // #ef4444
            border: Color::Rgb(30, 41, 59),                // #1e293b
            highlight: Color::Rgb(14, 165, 233),           // #0ea5e9
            container: Color::Rgb(30, 41, 59),             // #1e293b
        }
    }

    pub fn forest_depths() -> Self {
        Self {
            background: Color::Rgb(26, 47, 31),            // #1a2f1f
            primary: Color::Rgb(92, 139, 97),              // #5c8b61
            secondary: Color::Rgb(45, 74, 51),             // #2d4a33
            text: Color::Rgb(201, 228, 202),               // #c9e4ca
            text_secondary: Color::Rgb(143, 187, 145),     // #8fbb91
            accent: Color::Rgb(127, 182, 133),             // #7fb685
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Rgb(230, 124, 115),              // #e67c73
            border: Color::Rgb(45, 74, 51),                // #2d4a33
            highlight: Color::Rgb(92, 139, 97),            // #5c8b61
            container: Color::Rgb(45, 74, 51),             // #2d4a33
        }
    }

    pub fn sunset_horizon() -> Self {
        Self {
            background: Color::Rgb(43, 28, 44),            // #2b1c2c
            primary: Color::Rgb(232, 135, 92),             // #e8875c
            secondary: Color::Rgb(67, 46, 68),             // #432e44
            text: Color::Rgb(255, 217, 192),               // #ffd9c0
            text_secondary: Color::Rgb(212, 165, 165),     // #d4a5a5
            accent: Color::Rgb(255, 158, 100),             // #ff9e64
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Rgb(255, 107, 107),              // #ff6b6b
            border: Color::Rgb(67, 46, 68),                // #432e44
            highlight: Color::Rgb(232, 135, 92),           // #e8875c
            container: Color::Rgb(67, 46, 68),             // #432e44
        }
    }

    pub fn arctic_frost() -> Self {
        Self {
            background: Color::Rgb(26, 29, 33),            // #1a1d21
            primary: Color::Rgb(94, 129, 172),             // #5e81ac
            secondary: Color::Rgb(42, 47, 54),             // #2a2f36
            text: Color::Rgb(236, 239, 244),               // #eceff4
            text_secondary: Color::Rgb(174, 179, 187),     // #aeb3bb
            accent: Color::Rgb(136, 192, 208),             // #88c0d0
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Rgb(191, 97, 106),               // #bf616a
            border: Color::Rgb(42, 47, 54),                // #2a2f36
            highlight: Color::Rgb(94, 129, 172),           // #5e81ac
            container: Color::Rgb(42, 47, 54),             // #2a2f36
        }
    }

    pub fn cyber_synthwave() -> Self {
        Self {
            background: Color::Rgb(26, 23, 33),            // #1a1721
            primary: Color::Rgb(179, 23, 119),             // #b31777
            secondary: Color::Rgb(42, 31, 58),             // #2a1f3a
            text: Color::Rgb(238, 230, 255),               // #eee6ff
            text_secondary: Color::Rgb(195, 183, 217),     // #c3b7d9
            accent: Color::Rgb(249, 42, 173),              // #f92aad
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Rgb(255, 46, 99),                // #ff2e63
            border: Color::Rgb(42, 31, 58),                // #2a1f3a
            highlight: Color::Rgb(179, 23, 119),           // #b31777
            container: Color::Rgb(42, 31, 58),             // #2a1f3a
        }
    }

    pub fn github_light() -> Self {
        Self {
            background: Color::Rgb(255, 255, 255),         // #ffffff
            primary: Color::Rgb(84, 163, 255),             // #54a3ff
            secondary: Color::Rgb(36, 41, 46),             // #24292e
            text: Color::Rgb(112, 119, 126),               // #70777e
            text_secondary: Color::Rgb(112, 115, 120),     // #707378
            accent: Color::Rgb(102, 109, 118),             // #666d76
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            border: Color::Black,
            highlight: Color::Rgb(213, 208, 226),          // #d5d0e2
            container: Color::Rgb(250, 251, 252),          // #fafbfc
        }
    }

    pub fn neon() -> Self {
        Self {
            background: Color::Rgb(18, 14, 22),            // #120e16
            primary: Color::Rgb(247, 92, 29),              // #f75c1d
            secondary: Color::Rgb(18, 14, 22),             // #120e16
            text: Color::Rgb(159, 157, 161),               // #9F9DA1
            text_secondary: Color::Rgb(146, 187, 117),     // #92bb75
            accent: Color::Rgb(74, 83, 94),                // #4a535e
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            border: Color::Black,
            highlight: Color::Rgb(112, 0, 255),            // #7000ff
            container: Color::Rgb(26, 23, 30),             // #1a171e
        }
    }

    pub fn kimbie() -> Self {
        Self {
            background: Color::Rgb(34, 26, 15),            // #221a0f
            primary: Color::Rgb(202, 152, 88),             // #ca9858
            secondary: Color::Rgb(19, 21, 16),             // #131510
            text: Color::Rgb(177, 173, 134),               // #B1AD86
            text_secondary: Color::Rgb(177, 173, 134),     // #B1AD86
            accent: Color::Rgb(74, 83, 94),                // #4a535e
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            border: Color::Black,
            highlight: Color::Rgb(211, 175, 134),          // #d3af86
            container: Color::Rgb(54, 39, 18),             // #362712
        }
    }

    pub fn gruvbox_light() -> Self {
        Self {
            background: Color::Rgb(249, 245, 215),         // #f9f5d7
            primary: Color::Rgb(209, 172, 14),             // #d1ac0e
            secondary: Color::Rgb(251, 241, 199),          // #fbf1c7
            text: Color::Rgb(95, 87, 80),                  // #5f5750
            text_secondary: Color::Rgb(172, 162, 137),     // #aca289
            accent: Color::Rgb(224, 219, 178),             // #e0dbb2
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            border: Color::Black,
            highlight: Color::Rgb(207, 210, 168),          // #cfd2a8
            container: Color::Rgb(251, 241, 199),          // #fbf1c7
        }
    }

    pub fn gruvbox_dark() -> Self {
        Self {
            background: Color::Rgb(50, 48, 47),            // #32302f
            primary: Color::Rgb(66, 67, 20),               // #424314
            secondary: Color::Rgb(40, 40, 40),             // #282828
            text: Color::Rgb(134, 135, 41),                // #868729
            text_secondary: Color::Rgb(134, 135, 41),      // #868729
            accent: Color::Rgb(235, 219, 178),             // #ebdbb2
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            border: Color::Black,
            highlight: Color::Rgb(89, 84, 74),             // #59544a
            container: Color::Rgb(48, 46, 46),             // #302e2e
        }
    }

    pub fn greenie_meanie() -> Self {
        Self {
            background: Color::Rgb(20, 46, 40),            // #142e28
            primary: Color::Rgb(34, 78, 68),               // #224e44
            secondary: Color::Rgb(41, 42, 46),             // #292A2E
            text: Color::Rgb(72, 157, 80),                 // #489D50
            text_secondary: Color::Rgb(72, 157, 80),       // #489D50
            accent: Color::Rgb(68, 100, 72),               // #446448
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            border: Color::Black,
            highlight: Color::Rgb(75, 85, 99),             // #4b5563
            container: Color::Rgb(41, 42, 46),             // #292A2E
        }
    }

    pub fn wildberries() -> Self {
        Self {
            background: Color::Rgb(36, 0, 65),             // #240041
            primary: Color::Rgb(75, 36, 107),              // #4b246b
            secondary: Color::Rgb(25, 0, 46),              // #19002E
            text: Color::Rgb(207, 139, 62),                // #CF8B3E
            text_secondary: Color::Rgb(207, 139, 62),      // #CF8B3E
            accent: Color::Rgb(199, 155, 255),             // #C79BFF
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            border: Color::Black,
            highlight: Color::Rgb(68, 67, 58),             // #44433A
            container: Color::Rgb(25, 0, 46),              // #19002E
        }
    }

    pub fn hot_dog_stand() -> Self {
        Self {
            background: Color::Rgb(103, 11, 10),           // #670b0a
            primary: Color::Rgb(213, 188, 92),             // #D5BC5C
            secondary: Color::Rgb(238, 185, 17),           // #EEB911
            text: Color::Rgb(18, 18, 21),                  // #121215
            text_secondary: Color::Rgb(213, 188, 92),      // #D5BC5C
            accent: Color::Rgb(213, 188, 92),              // #D5BC5C
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            border: Color::Black,
            highlight: Color::Rgb(75, 85, 99),             // #4b5563
            container: Color::Rgb(195, 89, 13),            // #C3590D
        }
    }

    pub fn from_name(theme_name: &str) -> Self {
        match theme_name.to_lowercase().as_str() {
            "light" => Self::light(),
            "soft lavender" => Self::soft_lavender(),
            "minty fresh" => Self::minty_fresh(),
            "warm vanilla" => Self::warm_vanilla(),
            "coastal blue" => Self::coastal_blue(),
            "paper cream" => Self::paper_cream(),
            "dark" => Self::dark(),
            "nordic light" => Self::nordic_light(),
            "nordic" => Self::nordic(),
            "abyss" => Self::abyss(),
            "dracula" => Self::dracula(),
            "catppuccin mocha mauve" => Self::catppuccin_mocha_mauve(),
            "midnight ocean" => Self::midnight_ocean(),
            "forest depths" => Self::forest_depths(),
            "sunset horizon" => Self::sunset_horizon(),
            "arctic frost" => Self::arctic_frost(),
            "cyber synthwave" => Self::cyber_synthwave(),
            "github light" => Self::github_light(),
            "neon" => Self::neon(),
            "kimbie" => Self::kimbie(),
            "gruvbox light" => Self::gruvbox_light(),
            "gruvbox dark" => Self::gruvbox_dark(),
            "greenie meanie" => Self::greenie_meanie(),
            "wildberries" => Self::wildberries(),
            "hot dog stand - my eyes" => Self::hot_dog_stand(),
            _ => {
                log::warn!("Unknown theme '{}', falling back to light", theme_name);
                Self::light()
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThemeManager {
    current_theme: ThemeColors,
    theme_name: String,
}

impl Default for ThemeManager {
    fn default() -> Self {
        Self {
            current_theme: ThemeColors::default(),
            theme_name: "Light".to_string(),
        }
    }
}

impl ThemeManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn fetch_theme_from_server(client: &crate::api::PinepodsClient, user_id: i32) -> Result<String> {
        let url = format!("/api/data/get_theme/{}", user_id);
        let response: ThemeResponse = client.authenticated_get(&url).await?;
        log::info!("Fetched theme from server: {}", response.theme);
        Ok(response.theme)
    }

    pub fn set_theme(&mut self, theme_name: &str) {
        self.theme_name = theme_name.to_string();
        self.current_theme = ThemeColors::from_name(theme_name);
        log::info!("Theme set to: {}", theme_name);
    }

    pub fn get_colors(&self) -> &ThemeColors {
        &self.current_theme
    }

    pub fn get_theme_name(&self) -> &str {
        &self.theme_name
    }

    pub fn available_themes() -> Vec<&'static str> {
        vec![
            "Light",
            "Soft Lavender",
            "Minty Fresh",
            "Warm Vanilla",
            "Coastal Blue",
            "Paper Cream",
            "Dark",
            "Nordic Light",
            "Nordic",
            "Abyss",
            "Dracula",
            "Catppuccin Mocha Mauve",
            "Midnight Ocean",
            "Forest Depths",
            "Sunset Horizon",
            "Arctic Frost",
            "Cyber Synthwave",
            "Github Light",
            "Neon",
            "Kimbie",
            "Gruvbox Light",
            "Gruvbox Dark",
            "Greenie Meanie",
            "Wildberries",
            "Hot Dog Stand - MY EYES",
        ]
    }
}

// Helper functions for common theme operations
pub fn get_list_item_style(theme: &ThemeColors, selected: bool) -> ratatui::style::Style {
    use ratatui::style::{Modifier, Style};
    
    if selected {
        Style::default()
            .bg(theme.highlight)
            .fg(theme.background)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.text)
    }
}

pub fn get_border_style(theme: &ThemeColors) -> ratatui::style::Style {
    ratatui::style::Style::default().fg(theme.border)
}

pub fn get_title_style(theme: &ThemeColors) -> ratatui::style::Style {
    use ratatui::style::{Modifier, Style};
    Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD)
}