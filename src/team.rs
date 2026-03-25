pub struct Team {
    name: String,
}

impl Team {
    pub fn new(name: &str) -> Team {
        Team {
            name: name.to_string(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone)]
pub struct Batter {
    name: String,
    order: i8,
}

impl Batter {
    pub fn new(name: &str, order: i8) -> Batter {
        Batter {
            name: name.to_string(),
            order: order,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn order(&self) -> &i8 {
        &self.order
    }
}
