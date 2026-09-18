/// Renders a complete, deterministic starter configuration with no adapters enabled.
pub struct ConfigSkeleton;

/// Identity supplied to project initialization when no configuration exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectIdentity {
    name: String,
    canonical_repository: String,
    right_holders: Vec<String>,
}

impl ProjectIdentity {
    pub fn new(
        name: impl Into<String>,
        canonical_repository: impl Into<String>,
        right_holders: Vec<String>,
    ) -> Self {
        Self {
            name: name.into(),
            canonical_repository: canonical_repository.into(),
            right_holders,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn canonical_repository(&self) -> &str {
        &self.canonical_repository
    }

    pub fn right_holders(&self) -> &[String] {
        &self.right_holders
    }
}

impl ConfigSkeleton {
    pub fn render() -> &'static str {
        concat!(
            "# AHCL Kit project configuration.\n",
            "schema = 1\n",
            "materials-directory = \".ahcl\"\n",
            "languages = []\n",
            "\n",
            "[project]\n",
            "name = \"\"\n",
            "canonical-repository = \"\"\n",
            "canonical-branch = \"master\"\n",
            "right-holders = []\n",
            "contact = \"\"\n",
            "adoption-date = \"\"\n",
            "\n",
            "[license]\n",
            "version = \"1.1\"\n",
            "special-authorization-channel = \"\"\n",
            "\n",
            "[generation]\n",
            "strict-license-files = true\n",
            "\n",
            "# To enable the Rust adapter, replace `languages = []` above with:\n",
            "# languages:\n",
            "#   - \"rust\"\n",
            "#\n",
            "# Then add:\n",
            "# [rust.cargo]\n",
            "# manifests:\n",
            "#   - \"Cargo.toml\"\n",
            "# packages = []\n",
            "# rules = []\n",
        )
    }

    /// Renders a complete no-language configuration from project-init flags.
    pub fn render_populated(identity: &ProjectIdentity) -> String {
        let mut rendered = String::from(
            "schema = 1\nmaterials-directory = \".ahcl\"\nlanguages = []\n\n[project]\n",
        );
        rendered.push_str("name = ");
        rendered.push_str(&quoted(identity.name()));
        rendered.push_str("\ncanonical-repository = ");
        rendered.push_str(&quoted(identity.canonical_repository()));
        rendered.push_str("\ncanonical-branch = \"master\"\n");
        if identity.right_holders().is_empty() {
            rendered.push_str("right-holders = []\n");
        } else {
            rendered.push_str("right-holders:");
            for holder in identity.right_holders() {
                rendered.push_str("\n  - ");
                rendered.push_str(&quoted(holder));
            }
            rendered.push('\n');
        }
        rendered.push_str(
            "contact = \"\"\nadoption-date = \"\"\n\n[license]\nversion = \"1.1\"\nspecial-authorization-channel = \"\"\n\n[generation]\nstrict-license-files = true\n",
        );
        rendered
    }
}

fn quoted(value: &str) -> String {
    let mut rendered = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => rendered.push_str("\\\\"),
            '\"' => rendered.push_str("\\\""),
            '\n' => rendered.push_str("\\n"),
            '\r' => rendered.push_str("\\r"),
            '\t' => rendered.push_str("\\t"),
            _ => rendered.push(character),
        }
    }
    rendered.push('\"');
    rendered
}
