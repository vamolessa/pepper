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

pub struct SectionIterator<'ini, 'source> {
    source: &'source str,
    ini: &'ini Ini,
    index: usize,
}
impl<'ini, 'source> Iterator for SectionIterator<'ini, 'source> {
    type Item = (&'source str, usize, PropertyIterator<'ini, 'source>);
    fn next(&mut self) -> Option<Self::Item> {
        let section = self.ini.sections.get(self.index)?;
        self.index += 1;
        let name = &self.source[section.name.start as usize..section.name.end as usize];
        let properties_range = section.properties.start as usize..section.properties.end as usize;
        let properties = PropertyIterator {
            source: self.source,
            properties: &self.ini.properties[properties_range],
        };

        Some((name, section.line_index as _, properties))
    }
}

pub struct PropertyIterator<'ini, 'source> {
    source: &'source str,
    properties: &'ini [Property],
}
impl<'ini, 'source> Iterator for PropertyIterator<'ini, 'source> {
    type Item = (&'source str, &'source str, usize);
    fn next(&mut self) -> Option<Self::Item> {
        match self.properties {
            [] => None,
            [property, rest @ ..] => {
                let &Property {
                    ref key,
                    ref value,
                    line_index,
                } = property;

                self.properties = rest;
                let key = &self.source[key.start as usize..key.end as usize];
                let value = &self.source[value.start as usize..value.end as usize];

                Some((key, value, line_index as _))
            }
        }
    }
}

#[derive(Default)]
pub struct Ini {
    sections: Vec<Section>,
    properties: Vec<Property>,
}
impl Ini {
    pub fn parse<'this, 'source>(
        &'this mut self,
        source: &'source str,
    ) -> Result<SectionIterator<'this, 'source>, IniError> {
        fn get_range(source: &str, sub: &str) -> Range<u32> {
            let start = sub.as_ptr() as u32 - source.as_ptr() as u32;
            let end = start + sub.len() as u32;
            start..end
        }

        self.sections.clear();
        self.properties.clear();

        for (i, line) in source.lines().enumerate() {
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
                            name: get_range(source, name),
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
                        key: get_range(source, key),
                        value: get_range(source, value),
                        line_index: i as _,
                    });
                }
            }
        }

        if let Some(section) = self.sections.last_mut() {
            section.properties.end = self.properties.len() as _;
        }

        Ok(SectionIterator {
            source,
            ini: self,
            index: 0,
        })
    }
}

struct Section {
    pub name: Range<u32>,
    pub line_index: u32,
    pub properties: Range<u32>,
}

struct Property {
    pub key: Range<u32>,
    pub value: Range<u32>,
    pub line_index: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid() {
        fn get_sections<'parser, 'source>(
            parser: &'parser mut Ini,
            ini: &'source str,
        ) -> SectionIterator<'parser, 'source> {
            match parser.parse(ini) {
                Ok(sections) => sections,
                Err(error) => panic!("{} at line {}", error.kind, error.line_index + 1),
            }
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

        let (name, _, mut properties) = sections.next().unwrap();
        assert_eq!("sec0", name);
        assert_eq!(Some(("key0", "value0", 3)), properties.next());
        assert_eq!(Some(("key2", "", 5)), properties.next());
        assert_eq!(None, properties.next());

        let (name, _, mut properties) = sections.next().unwrap();
        assert_eq!(" sec2 ", name);
        assert_eq!(Some(("key3", ";value3", 9)), properties.next());
        assert_eq!(None, properties.next());

        let (name, _, mut properties) = sections.next().unwrap();
        assert_eq!("sec3", name);
        assert_eq!(None, properties.next());

        assert!(sections.next().is_none());
    }

    #[test]
    fn invalid() {
        fn get_error(ini: &str) -> IniErrorKind {
            let mut parser = Ini::default();
            match parser.parse(ini) {
                Ok(_) => panic!("ini parsed successfully"),
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
