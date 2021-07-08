use std::{fmt, ops::Range};

pub enum IniErrorKind {
    ExpectedSection,
    ExpectedEquals,
    ExpectedCloseSquareBrackets,
    SectionNotEndedWithCloseSquareBrackets,
    EmptySectionName,
    EmptyPropertyName,
}
impl fmt::Display for IniErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::ExpectedSection => f.write_str("expected section"),
            Self::ExpectedEquals => f.write_str("expected '='"),
            Self::ExpectedCloseSquareBrackets => f.write_str("expected ']'"),
            Self::SectionNotEndedWithCloseSquareBrackets => {
                f.write_str("section did not end with ']'")
            }
            Self::EmptySectionName => f.write_str("sections can not have an empty name"),
            Self::EmptyPropertyName => f.write_str("properties can not have an empty name"),
        }
    }
}

pub struct IniError {
    pub kind: IniErrorKind,
    pub line_index: usize,
}

pub struct SectionIterator<'ini, 'data> {
    ini: &'ini Ini<'data>,
    index: usize,
}
impl<'ini, 'data> Iterator for SectionIterator<'ini, 'data> {
    type Item = (&'data str, usize, PropertyIterator<'ini, 'data>);
    fn next(&mut self) -> Option<Self::Item> {
        let section = self.ini.sections.get(self.index)?;
        self.index += 1;
        let properties_range = section.properties.start as usize..section.properties.end as usize;
        let properties = PropertyIterator {
            properties: &self.ini.properties[properties_range],
        };
        Some((section.name, section.line_index as _, properties))
    }
}

pub struct PropertyIterator<'ini, 'data> {
    properties: &'ini [Property<'data>],
}
impl<'ini, 'data> Iterator for PropertyIterator<'ini, 'data> {
    type Item = (&'data str, &'data str, usize);
    fn next(&mut self) -> Option<Self::Item> {
        match self.properties {
            [] => None,
            [property, rest @ ..] => {
                let &Property {
                    key,
                    value,
                    line_index,
                } = property;
                self.properties = rest;
                Some((key, value, line_index as _))
            }
        }
    }
}

#[derive(Default)]
pub struct Ini<'a> {
    sections: Vec<Section<'a>>,
    properties: Vec<Property<'a>>,
}
impl<'a> Ini<'a> {
    pub fn parse(&mut self, text: &'a str) -> Result<(), IniError> {
        self.sections.clear();
        self.properties.clear();

        for (i, line) in text.lines().enumerate() {
            if line.is_empty() || line.starts_with(';') {
                continue;
            }

            match line.strip_prefix('[') {
                Some(rest) => match rest.find(']') {
                    Some(0) => {
                        return Err(IniError {
                            kind: IniErrorKind::EmptySectionName,
                            line_index: i,
                        });
                    }
                    Some(j) => {
                        let (name, rest) = rest.split_at(j);
                        if rest.len() > 1 {
                            return Err(IniError {
                                kind: IniErrorKind::SectionNotEndedWithCloseSquareBrackets,
                                line_index: i,
                            });
                        }

                        let start = self.properties.len() as _;

                        if let Some(section) = self.sections.last_mut() {
                            section.properties.end = start;
                        }

                        self.sections.push(Section {
                            name,
                            properties: start..start,
                            line_index: i as _,
                        });
                    }
                    None => {
                        return Err(IniError {
                            kind: IniErrorKind::ExpectedCloseSquareBrackets,
                            line_index: i,
                        });
                    }
                },
                None => {
                    if self.sections.is_empty() {
                        return Err(IniError {
                            kind: IniErrorKind::ExpectedSection,
                            line_index: i,
                        });
                    }

                    let (key, value) = match line.find('=') {
                        Some(0) => {
                            return Err(IniError {
                                kind: IniErrorKind::EmptyPropertyName,
                                line_index: i,
                            });
                        }
                        Some(j) => {
                            let key = &line[..j];
                            let value = &line[j + 1..];
                            (key, value)
                        }
                        None => {
                            return Err(IniError {
                                kind: IniErrorKind::ExpectedEquals,
                                line_index: i,
                            });
                        }
                    };

                    self.properties.push(Property {
                        key,
                        value,
                        line_index: i as _,
                    });
                }
            }
        }

        if let Some(section) = self.sections.last_mut() {
            section.properties.end = self.properties.len() as _;
        }

        Ok(())
    }

    pub fn sections<'this>(&'this self) -> SectionIterator<'this, 'a> {
        SectionIterator {
            ini: self,
            index: 0,
        }
    }
}

struct Section<'a> {
    pub name: &'a str,
    pub line_index: u32,
    pub properties: Range<u32>,
}

struct Property<'a> {
    pub key: &'a str,
    pub value: &'a str,
    pub line_index: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid() {
        fn get_sections<'parser, 'data>(
            parser: &'parser mut Ini<'data>,
            ini: &'data str,
        ) -> SectionIterator<'parser, 'data> {
            if let Err(error) = parser.parse(ini) {
                panic!("{} at line {}", error.kind, error.line_index + 1);
            }
            parser.sections()
        }

        let mut parser = Ini::default();

        let mut sections = get_sections(&mut parser, "");
        assert!(sections.next().is_none());

        let mut sections = get_sections(
            &mut parser,
            concat!(
                "\n",
                "; comment\n",
                "[sec0]\n",
                "key0=value0\n",
                ";key1=commented\n",
                "key2=\n",
                "\n",
                ";[sec1]\n",
                "[ sec2 ]\n",
                "key3=;value3\n",
                "[sec3]\n",
                "\n",
            ),
        );

        let (name, mut properties) = sections.next().unwrap();
        assert_eq!("sec0", name);
        assert_eq!(Some(("key0", "value0")), properties.next());
        assert_eq!(Some(("key2", "")), properties.next());
        assert_eq!(None, properties.next());

        let (name, mut properties) = sections.next().unwrap();
        assert_eq!(" sec2 ", name);
        assert_eq!(Some(("key3", ";value3")), properties.next());
        assert_eq!(None, properties.next());

        let (name, mut properties) = sections.next().unwrap();
        assert_eq!("sec3", name);
        assert_eq!(None, properties.next());

        assert!(sections.next().is_none());
    }

    #[test]
    fn invalid() {
        fn get_error(ini: &str) -> IniErrorKind {
            let mut parser = Ini::default();
            match parser.parse(ini) {
                Ok(()) => panic!("ini parsed successfully"),
                Err(error) => error.kind,
            }
        }

        assert!(matches!(get_error("a=b"), IniErrorKind::ExpectedSection));
        assert!(matches!(
            get_error("[section]\na"),
            IniErrorKind::ExpectedEquals
        ));
        assert!(matches!(
            get_error("[section"),
            IniErrorKind::ExpectedCloseSquareBrackets
        ));
        assert!(matches!(
            get_error("[section] "),
            IniErrorKind::SectionNotEndedWithCloseSquareBrackets
        ));
        assert!(matches!(get_error("[]"), IniErrorKind::EmptySectionName));
        assert!(matches!(
            get_error("[section]\n=b"),
            IniErrorKind::EmptyPropertyName,
        ));
    }
}

