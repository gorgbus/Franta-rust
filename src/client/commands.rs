use serde::{self, Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
pub struct NameLocalization {
    pub cs: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct DescLocalization {
    pub cs: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct ApplicationCommand {
    #[serde(rename = "type")]
    pub command_type: u32,
    pub name: String,
    pub description: String,
    pub options: Vec<ApplicationCommandOption>,
    pub name_localizations: Option<NameLocalization>,
    pub description_localizations: Option<DescLocalization>,
}

impl ApplicationCommand {
    pub fn new(command_type: u32, name: String, description: String) -> Self {
        Self {
            command_type,
            name,
            description,
            options: vec![],
            name_localizations: None,
            description_localizations: None,
        }
    }

    pub fn add_option(&mut self, option: ApplicationCommandOption) {
        self.options.push(option);
    }

    pub fn set_name_loc(mut self, name: &str) -> Self {
        self.name_localizations = Some(NameLocalization {
            cs: String::from(name),
        });

        self
    }

    pub fn set_desc_loc(mut self, desc: &str) -> Self {
        self.description_localizations = Some(DescLocalization {
            cs: String::from(desc),
        });

        self
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub struct ApplicationCommandOption {
    pub name: String,
    pub description: String,
    pub name_localizations: Option<NameLocalization>,
    pub description_localizations: Option<DescLocalization>,
    #[serde(rename = "type")]
    pub option_type: u32,
    pub required: bool,
    pub choices: Vec<ApplicationCommandOptionChoice>,
    pub options: Vec<ApplicationCommandOption>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct ApplicationCommandOptionChoice {
    pub name: String,
    pub value: String,
}

impl ApplicationCommandOption {
    pub fn new(name: String, description: String, option_type: u32, required: bool) -> Self {
        Self {
            name,
            name_localizations: None,
            description,
            description_localizations: None,
            option_type,
            required,
            choices: vec![],
            options: vec![],
        }
    }

    pub fn add_choice(&mut self, choice: ApplicationCommandOptionChoice) {
        self.choices.push(choice);
    }

    pub fn add_option(&mut self, option: ApplicationCommandOption) {
        self.options.push(option);
    }

    pub fn set_name_loc(mut self, name: &str) -> Self {
        self.name_localizations = Some(NameLocalization {
            cs: String::from(name),
        });

        self
    }

    pub fn set_desc_loc(mut self, desc: &str) -> Self {
        self.description_localizations = Some(DescLocalization {
            cs: String::from(desc),
        });

        self
    }
}
