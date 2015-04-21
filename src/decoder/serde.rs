use serde::de;
use Value;
use super::{Decoder, DecodeError, DecodeErrorKind};
use std::collections::BTreeMap;

struct MapVisitor<'a, I> {
    iter: I,
    toml: &'a mut Option<Value>,
    key: Option<String>,
    value: Option<Value>,
}

fn se2toml(err: de::value::Error, ty: &'static str) -> DecodeError {
    match err {
        de::value::Error::SyntaxError => de::Error::syntax_error(),
        de::value::Error::EndOfStreamError => de::Error::end_of_stream_error(),
        de::value::Error::MissingFieldError(s) => {
            DecodeError {
                field: Some(s.to_string()),
                kind: DecodeErrorKind::ExpectedField(Some(ty)),
            }
        },
        de::value::Error::UnknownFieldError(s) => {
            DecodeError {
                field: Some(s.to_string()),
                kind: DecodeErrorKind::UnknownField,
            }
        },
    }
}

impl de::Deserializer for Decoder {
    type Error = DecodeError;

    fn visit<V>(&mut self, mut visitor: V) -> Result<V::Value, DecodeError>
        where V: de::Visitor
    {
        match self.toml.take() {
            Some(Value::String(s)) => {
                visitor.visit_string(s).map_err(|e| se2toml(e, "string"))
            }
            Some(Value::Integer(i)) => {
                visitor.visit_i64(i).map_err(|e| se2toml(e, "integer"))
            }
            Some(Value::Float(f)) => {
                visitor.visit_f64(f).map_err(|e| se2toml(e, "float"))
            }
            Some(Value::Boolean(b)) => {
                visitor.visit_bool(b).map_err(|e| se2toml(e, "bool"))
            }
            Some(Value::Datetime(s)) => {
                visitor.visit_string(s).map_err(|e| se2toml(e, "date"))
            }
            Some(Value::Array(a)) => {
                let len = a.len();
                let iter = a.into_iter();
                visitor.visit_seq(SeqDeserializer::new(iter, len, &mut self.toml))
            }
            Some(Value::Table(t)) => {
                visitor.visit_map(MapVisitor {
                    iter: t.into_iter(),
                    toml: &mut self.toml,
                    key: None,
                    value: None,
                })
            }
            None => Err(de::Error::end_of_stream_error()),
        }
    }

    fn visit_option<V>(&mut self, mut visitor: V) -> Result<V::Value, DecodeError>
        where V: de::Visitor
    {
        if self.toml.is_none() {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn visit_seq<V>(&mut self, mut visitor: V) -> Result<V::Value, DecodeError>
        where V: de::Visitor,
    {
        if self.toml.is_none() {
            let iter = None::<i32>.into_iter();
            let e = visitor.visit_seq(de::value::SeqDeserializer::new(iter, 0));
            e.map_err(|e| se2toml(e, "array"))
        } else {
            self.visit(visitor)
        }
    }
}

struct SeqDeserializer<'a, I> {
    iter: I,
    len: usize,
    toml: &'a mut Option<Value>,
}

impl<'a, I> SeqDeserializer<'a, I> where I: Iterator<Item=Value> {
    fn new(iter: I, len: usize, toml: &'a mut Option<Value>) -> Self {
        SeqDeserializer {
            iter: iter,
            len: len,
            toml: toml,
        }
    }

    fn put_value_back(&mut self, v: Value) {
        *self.toml = self.toml.take().or(Some(Value::Array(Vec::new())));
        match self.toml.as_mut().unwrap() {
            &mut Value::Array(ref mut a) => {
                a.push(v);
            },
            _ => unreachable!(),
        }
    }
}

impl<'a, I> de::Deserializer for SeqDeserializer<'a, I>
    where I: Iterator<Item=Value>,
{
    type Error = DecodeError;

    fn visit<V>(&mut self, mut visitor: V) -> Result<V::Value, DecodeError>
        where V: de::Visitor,
    {
        visitor.visit_seq(self)
    }
}

impl<'a, I> de::SeqVisitor for SeqDeserializer<'a, I>
    where I: Iterator<Item=Value>
{
    type Error = DecodeError;

    fn visit<V>(&mut self) -> Result<Option<V>, DecodeError>
        where V: de::Deserialize
    {
        match self.iter.next() {
            Some(value) => {
                self.len -= 1;
                let mut de = Decoder::new(value);
                let v = try!(de::Deserialize::deserialize(&mut de));
                if let Some(t) = de.toml {
                    self.put_value_back(t);
                }
                Ok(Some(v))
            }
            None => Ok(None),
        }
    }

    fn end(&mut self) -> Result<(), DecodeError> {
        if self.len == 0 {
            Ok(())
        } else {
            Err(de::Error::end_of_stream_error())
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl de::Error for DecodeError {
    fn syntax_error() -> DecodeError {
        DecodeError { field: None, kind: DecodeErrorKind::SyntaxError }
    }
    fn end_of_stream_error() -> DecodeError {
        DecodeError { field: None, kind: DecodeErrorKind::EndOfStream }
    }
    fn missing_field_error(name: &'static str) -> DecodeError {
        DecodeError {
            field: Some(name.to_string()),
            kind: DecodeErrorKind::ExpectedField(None),
        }
    }
    fn unknown_field_error(name: &str) -> DecodeError {
        DecodeError {
            field: Some(name.to_string()),
            kind: DecodeErrorKind::UnknownField,
        }
    }
}

impl<'a, I> MapVisitor<'a, I> {
    fn put_value_back(&mut self, v: Value) {
        *self.toml = self.toml.take().or_else(|| {
            Some(Value::Table(BTreeMap::new()))
        });
        match self.toml.as_mut().unwrap() {
            &mut Value::Table(ref mut t) => {
                t.insert(self.key.take().unwrap(), v);
            },
            _ => unreachable!(),
        }
    }
}

impl<'a, I> de::MapVisitor for MapVisitor<'a, I>
    where I: Iterator<Item=(String, Value)>
{
    type Error = DecodeError;

    fn visit_key<K>(&mut self) -> Result<Option<K>, DecodeError>
        where K: de::Deserialize
    {
        while let Some((k, v)) = self.iter.next() {
            self.key = Some(k.clone());
            let mut dec = Decoder::new(Value::String(k));
            match de::Deserialize::deserialize(&mut dec) {
                Ok(val) => {
                    self.value = Some(v);
                    return Ok(Some(val))
                }

                // If this was an unknown field, then we put the toml value
                // back into the map and keep going.
                Err(DecodeError {kind: DecodeErrorKind::UnknownField, ..}) => {
                    self.put_value_back(v);
                }
                Err(e) => return Err(e),
            }
        }
        Ok(None)
    }

    fn visit_value<V>(&mut self) -> Result<V, DecodeError>
        where V: de::Deserialize
    {
        match self.value.take() {
            Some(t) => {
                let mut dec = Decoder::new(t);
                let v = try!(de::Deserialize::deserialize(&mut dec));
                if let Some(t) = dec.toml {
                    self.put_value_back(t);
                }
                Ok(v)
            },
            None => Err(de::Error::end_of_stream_error())
        }
    }

    fn end(&mut self) -> Result<(), DecodeError> {
        Ok(())
    }

    fn missing_field<V>(&mut self, field_name: &'static str)
                        -> Result<V, DecodeError> where V: de::Deserialize {
        // See if the type can deserialize from a unit.
        match de::Deserialize::deserialize(&mut UnitDeserializer) {
            Err(DecodeError {
                kind: DecodeErrorKind::SyntaxError,
                field,
            }) => Err(DecodeError {
                field: field.or(Some(field_name.to_string())),
                kind: DecodeErrorKind::ExpectedField(None),
            }),
            v => v,
        }
    }
}

struct UnitDeserializer;

impl de::Deserializer for UnitDeserializer {
    type Error = DecodeError;

    fn visit<V>(&mut self, mut visitor: V) -> Result<V::Value, DecodeError>
        where V: de::Visitor,
    {
        visitor.visit_unit()
    }

    fn visit_option<V>(&mut self, mut visitor: V) -> Result<V::Value, DecodeError>
        where V: de::Visitor,
    {
        visitor.visit_none()
    }
}