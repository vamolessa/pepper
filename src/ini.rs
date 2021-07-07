use std::ops::Range;

use crate::buffer_position::BufferPosition;

#[derive(Debug)]
pub enum IniErrorKind {
    ExpectedSection,
    ExpectedEquals,
    ExpectedCloseSquareBrackets,
    SectionNotEndedWithCloseSquareBrackets,
    EmptySectionName,
    EmptyPropertyName,
}
#[derive(Debug)]
pub struct IniError {
    pub kind: IniErrorKind,
    pub position: BufferPosition,
}

pub struct SectionIterator<'ini, 'data> {
    ini: &'ini Ini<'data>,
    index: usize,
}
impl<'ini, 'data> Iterator for SectionIterator<'ini, 'data> {
    type Item = (&'data str, PropertyIterator<'ini, 'data>);
    fn next(&mut self) -> Option<Self::Item> {
        let section = self.ini.sections.get(self.index)?;
        self.index += 1;
        let properties_range = section.properties.start as usize..section.properties.end as usize;
        let properties = PropertyIterator {
            properties: &self.ini.properties[properties_range],
        };
        Some((section.name, properties))
    }
}

pub struct PropertyIterator<'ini, 'data> {
    properties: &'ini [Property<'data>],
}
impl<'ini, 'data> Iterator for PropertyIterator<'ini, 'data> {
    type Item = (&'data str, &'data str);
    fn next(&mut self) -> Option<Self::Item> {
        match self.properties {
            [] => None,
            [property, rest @ ..] => {
                let &Property { key, value } = property;
                self.properties = rest;
                Some((key, value))
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
    pub fn clear(&mut self) {
        self.sections.clear();
        self.properties.clear();
    }

    pub fn parse(&mut self, text: &'a str) -> Result<(), IniError> {
        for (i, line) in text.lines().enumerate() {
            if line.is_empty() || line.starts_with(';') {
                continue;
            }

            match line.strip_prefix('[') {
                Some(rest) => match rest.find(']') {
                    Some(0) => {
                        return Err(IniError {
                            kind: IniErrorKind::EmptySectionName,
                            position: BufferPosition::line_col(i as _, 1),
                        });
                    }
                    Some(j) => {
                        let (name, rest) = rest.split_at(j);
                        if rest.len() > 1 {
                            return Err(IniError {
                                kind: IniErrorKind::SectionNotEndedWithCloseSquareBrackets,
                                position: BufferPosition::line_col(i as _, (j + 1) as _),
                            });
                        }

                        let start = self.properties.len() as _;

                        if let Some(section) = self.sections.last_mut() {
                            section.properties.end = start;
                        }

                        self.sections.push(Section {
                            name,
                            properties: start..start,
                        });
                    }
                    None => {
                        return Err(IniError {
                            kind: IniErrorKind::ExpectedCloseSquareBrackets,
                            position: BufferPosition::line_col(i as _, (rest.len() + 1) as _),
                        });
                    }
                },
                None => {
                    if self.sections.is_empty() {
                        return Err(IniError {
                            kind: IniErrorKind::ExpectedSection,
                            position: BufferPosition::line_col(i as _, 0),
                        });
                    }

                    let (key, value) = match line.find('=') {
                        Some(0) => {
                            return Err(IniError {
                                kind: IniErrorKind::EmptyPropertyName,
                                position: BufferPosition::line_col(i as _, 0),
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
                                position: BufferPosition::line_col(i as _, line.len() as _),
                            });
                        }
                    };

                    self.properties.push(Property { key, value });
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
    pub properties: Range<u32>,
}

struct Property<'a> {
    pub key: &'a str,
    pub value: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn valid() {
        let ini = concat!(
            "; comment\n",
            "[sec0]\n",
            "key0=value0\n",
            ";key1=commented\n",
            "key2=\n",
            ";[sec1]\n",
            "[ sec2 ]\n",
            "key3=;value3\n",
            "[sec3]\n",
        );

        let mut parser = Ini::default();
        if let Err(error) = parser.parse(ini) {
            panic!("{:?}", error);
        }

        let mut sections = parser.sections();

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
}
