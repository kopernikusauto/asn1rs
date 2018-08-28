use codegen::Block;
use codegen::Function;
use codegen::Impl;
use codegen::Scope;

use model::Definition;
use model::Field;
use model::Model;
use model::ProtobufType;
use model::Role;

use io::protobuf::Format as ProtobufFormat;

const KEYWORDS: [&str; 9] = [
    "use", "mod", "const", "type", "pub", "enum", "struct", "impl", "trait",
];

#[derive(Debug)]
pub enum Error {}

pub struct Generator {
    models: Vec<Model>,
}

impl Generator {
    pub fn new() -> Generator {
        Generator { models: Vec::new() }
    }

    pub fn add_model(&mut self, model: Model) {
        self.models.push(model);
    }

    pub fn to_string(&self) -> Result<Vec<(String, String)>, Error> {
        let mut files = Vec::new();
        for model in self.models.iter() {
            files.push(Self::model_to_file(
                model,
                &[&UperGenerator, &ProtobufGenerator],
            )?);
        }
        Ok(files)
    }

    pub fn model_to_file(
        model: &Model,
        generators: &[&SerializableGenerator],
    ) -> Result<(String, String), Error> {
        let file = {
            let mut string = Self::rust_module_name(&model.name);
            string.push_str(".rs");
            string
        };

        let mut scope = Scope::new();
        generators.iter().for_each(|g| g.add_imports(&mut scope));

        for import in model.imports.iter() {
            let from = format!("super::{}", Self::rust_module_name(&import.from));
            for what in import.what.iter() {
                scope.import(&from, &what);
            }
        }

        for definition in model.definitions.iter() {
            let name: String = match definition {
                Definition::SequenceOf(name, role) => {
                    let rust_type = role.clone().into_rust().to_string();
                    Self::new_struct(&mut scope, name)
                        .field("values", format!("Vec<{}>", rust_type));
                    {
                        scope
                            .new_impl(&name)
                            .impl_trait("::std::ops::Deref")
                            .associate_type("Target", format!("Vec<{}>", rust_type))
                            .new_fn("deref")
                            .arg_ref_self()
                            .ret(&format!("&Vec<{}>", rust_type))
                            .line(format!("&self.values"));
                    }
                    {
                        scope
                            .new_impl(&name)
                            .impl_trait("::std::ops::DerefMut")
                            .new_fn("deref_mut")
                            .arg_mut_self()
                            .ret(&format!("&mut Vec<{}>", rust_type))
                            .line(format!("&mut self.values"));
                    }
                    {
                        let implementation = scope.new_impl(&name);
                        {
                            implementation
                                .new_fn("values")
                                .vis("pub")
                                .ret(format!("&Vec<{}>", rust_type))
                                .arg_ref_self()
                                .line("&self.values");
                        }
                        {
                            implementation
                                .new_fn("values_mut")
                                .vis("pub")
                                .ret(format!("&mut Vec<{}>", rust_type))
                                .arg_mut_self()
                                .line("&mut self.values");
                        }
                        {
                            implementation
                                .new_fn("set_values")
                                .vis("pub")
                                .arg_mut_self()
                                .arg("values", format!("Vec<{}>", rust_type))
                                .line("self.values = values;");
                        }
                        Self::add_min_max_methods_if_applicable(implementation, "value", &role);
                    }
                    name.clone()
                }
                Definition::Sequence(name, fields) => {
                    {
                        let mut new_struct = Self::new_struct(&mut scope, name);
                        for field in fields.iter() {
                            new_struct.field(
                                &Self::rust_field_name(&field.name, true),
                                if field.optional {
                                    format!(
                                        "Option<{}>",
                                        field.role.clone().into_rust().to_string()
                                    )
                                } else {
                                    field.role.clone().into_rust().to_string()
                                },
                            );
                        }
                    }
                    {
                        let implementation = scope.new_impl(name);

                        for field in fields.iter() {
                            implementation
                                .new_fn(&Self::rust_field_name(&field.name, true))
                                .vis("pub")
                                .arg_ref_self()
                                .ret(if field.optional {
                                    format!(
                                        "&Option<{}>",
                                        field.role.clone().into_rust().to_string()
                                    )
                                } else {
                                    format!("&{}", field.role.clone().into_rust().to_string())
                                })
                                .line(format!(
                                    "&self.{}",
                                    Self::rust_field_name(&field.name, true)
                                ));

                            implementation
                                .new_fn(&format!(
                                    "{}_mut",
                                    Self::rust_field_name(&field.name, false)
                                ))
                                .vis("pub")
                                .arg_mut_self()
                                .ret(if field.optional {
                                    format!(
                                        "&mut Option<{}>",
                                        field.role.clone().into_rust().to_string()
                                    )
                                } else {
                                    format!("&mut {}", field.role.clone().into_rust().to_string())
                                })
                                .line(format!(
                                    "&mut self.{}",
                                    Self::rust_field_name(&field.name, true)
                                ));

                            implementation
                                .new_fn(&format!(
                                    "set_{}",
                                    Self::rust_field_name(&field.name, false)
                                ))
                                .vis("pub")
                                .arg_mut_self()
                                .arg(
                                    "value",
                                    if field.optional {
                                        format!(
                                            "Option<{}>",
                                            field.role.clone().into_rust().to_string()
                                        )
                                    } else {
                                        field.role.clone().into_rust().to_string()
                                    },
                                )
                                .line(format!(
                                    "self.{} = value;",
                                    Self::rust_field_name(&field.name, true)
                                ));

                            Self::add_min_max_methods_if_applicable(
                                implementation,
                                &field.name,
                                &field.role,
                            );
                        }
                    }
                    name.clone()
                }
                Definition::Enumeration(name, variants) => {
                    {
                        let mut enumeration = Self::new_enum(&mut scope, name);
                        for variant in variants.iter() {
                            enumeration.new_variant(&Self::rust_variant_name(&variant));
                        }
                    }
                    {
                        scope
                            .new_impl(&name)
                            .impl_trait("Default")
                            .new_fn("default")
                            .ret(&name as &str)
                            .line(format!(
                                "{}::{}",
                                name,
                                Self::rust_variant_name(&variants[0])
                            ));
                    }
                    {
                        let implementation = scope.new_impl(&name);
                        {
                            let values_fn = implementation
                                .new_fn("variants")
                                .vis("pub")
                                .ret(format!("[Self; {}]", variants.len()))
                                .line("[");

                            for variant in variants {
                                values_fn.line(format!(
                                    "{}::{},",
                                    name,
                                    Self::rust_variant_name(variant)
                                ));
                            }
                            values_fn.line("]");
                        }
                    }
                    name.clone()
                }
            };
            generators
                .iter()
                .for_each(|g| g.generate_implementations(&mut scope, &name, &definition));
        }

        Ok((file, scope.to_string()))
    }

    fn add_min_max_methods_if_applicable(implementation: &mut Impl, name: &str, role: &Role) {
        let min_max = match role {
            Role::Boolean => None,
            Role::Integer((lower, upper)) => Some((*lower, *upper)),
            Role::UnsignedMaxInteger => Some((0, ::std::i64::MAX)),
            Role::UTF8String => None,
            Role::Custom(_) => None,
        };

        if let Some((min, max)) = min_max {
            implementation
                .new_fn(&format!("{}_min", Self::rust_field_name(name, false)))
                .vis("pub")
                .ret(&role.clone().into_rust().to_string())
                .line(format!("{}", min));
            implementation
                .new_fn(&format!("{}_max", Self::rust_field_name(name, false)))
                .vis("pub")
                .ret(&role.clone().into_rust().to_string())
                .line(format!("{}", max));
        }
    }

    fn rust_field_name(name: &str, check_for_keywords: bool) -> String {
        let mut name = name.replace("-", "_");
        if check_for_keywords {
            for keyword in KEYWORDS.iter() {
                if keyword.eq(&name) {
                    name.push_str("_");
                    return name;
                }
            }
        }
        name
    }

    fn rust_variant_name(name: &str) -> String {
        let mut out = String::new();
        let mut next_upper = true;
        for c in name.chars() {
            if next_upper {
                out.push_str(&c.to_uppercase().to_string());
                next_upper = false;
            } else if c == '-' {
                next_upper = true;
            } else {
                out.push(c);
            }
        }
        out
    }

    fn rust_module_name(name: &str) -> String {
        let mut out = String::new();
        let mut prev_lowered = false;
        let mut chars = name.chars().peekable();
        while let Some(c) = chars.next() {
            let mut lowered = false;
            if c.is_uppercase() {
                if !out.is_empty() {
                    if !prev_lowered {
                        out.push('_');
                    } else if let Some(next) = chars.peek() {
                        if next.is_lowercase() {
                            out.push('_');
                        }
                    }
                }
                lowered = true;
                out.push_str(&c.to_lowercase().to_string());
            } else if c == '-' {
                out.push('_');
            } else {
                out.push(c);
            }
            prev_lowered = lowered;
        }
        out
    }

    fn new_struct<'a>(scope: &'a mut Scope, name: &str) -> &'a mut ::codegen::Struct {
        scope
            .new_struct(name)
            .vis("pub")
            .derive("Default")
            .derive("Debug")
            .derive("Clone")
            .derive("PartialEq")
    }

    fn new_enum<'a>(scope: &'a mut Scope, name: &str) -> &'a mut ::codegen::Enum {
        scope
            .new_enum(name)
            .vis("pub")
            .derive("Debug")
            .derive("Clone")
            .derive("Copy")
            .derive("PartialEq")
            .derive("PartialOrd")
    }

    fn new_serializable_impl<'a>(
        scope: &'a mut Scope,
        impl_for: &str,
        codec: &str,
    ) -> &'a mut Impl {
        scope.new_impl(impl_for).impl_trait(codec)
    }

    fn new_read_fn<'a>(implementation: &'a mut Impl, codec: &str) -> &'a mut Function {
        implementation
            .new_fn(&format!("read_{}", codec.to_lowercase()))
            .arg("reader", format!("&mut {}Reader", codec))
            .ret(format!("Result<Self, {}Error>", codec))
            .bound("Self", "Sized")
    }

    fn new_write_fn<'a>(implementation: &'a mut Impl, codec: &str) -> &'a mut Function {
        implementation
            .new_fn(&format!("write_{}", codec.to_lowercase()))
            .arg_ref_self()
            .arg("writer", format!("&mut {}Writer", codec))
            .ret(format!("Result<(), {}Error>", codec))
    }
}

pub trait SerializableGenerator {
    fn add_imports(&self, scope: &mut Scope);
    fn generate_implementations(&self, scope: &mut Scope, impl_for: &str, definition: &Definition);
}

pub struct UperGenerator;
impl SerializableGenerator for UperGenerator {
    fn add_imports(&self, scope: &mut Scope) {
        Self::add_imports(scope)
    }

    fn generate_implementations(&self, scope: &mut Scope, impl_for: &str, definition: &Definition) {
        Self::generate_implementations(scope, impl_for, definition)
    }
}

impl UperGenerator {
    const CODEC: &'static str = "Uper";

    fn new_uper_serializable_impl<'a>(scope: &'a mut Scope, impl_for: &str) -> &'a mut Impl {
        Generator::new_serializable_impl(scope, impl_for, Self::CODEC)
    }

    fn new_read_fn<'a>(implementation: &'a mut Impl) -> &'a mut Function {
        Generator::new_read_fn(implementation, Self::CODEC)
    }

    fn new_write_fn<'a>(implementation: &'a mut Impl) -> &'a mut Function {
        Generator::new_write_fn(implementation, Self::CODEC)
    }

    fn add_imports(scope: &mut Scope) {
        scope.import("asn1c::io::uper", Self::CODEC);
        scope.import("asn1c::io::uper", &format!("Error as {}Error", Self::CODEC));
        scope.import(
            "asn1c::io::uper",
            &format!("Reader as {}Reader", Self::CODEC),
        );
        scope.import(
            "asn1c::io::uper",
            &format!("Writer as {}Writer", Self::CODEC),
        );
    }

    fn generate_implementations(scope: &mut Scope, impl_for: &str, definition: &Definition) {
        let serializable_implementation = Self::new_uper_serializable_impl(scope, impl_for);
        match definition {
            Definition::SequenceOf(_name, aliased) => {
                {
                    let mut block = Self::new_write_fn(serializable_implementation);
                    block.line("writer.write_length_determinant(self.values.len())?;");
                    let mut block_for = Block::new("for value in self.values.iter()");
                    match aliased {
                        Role::Boolean => block_for.line("writer.write_bit(value)?;"),
                        Role::Integer(_) => block_for.line(format!(
                            "writer.write_int(*value as i64, (Self::value_min() as i64, Self::value_max() as i64))?;"
                        )),
                        Role::UnsignedMaxInteger => {
                            block_for.line("writer.write_int_max(*value)?;")
                        }
                        Role::Custom(_custom) => block_for.line("value.write_uper(writer)?;"),
                        Role::UTF8String => block_for.line("writer.write_utf8_string(&value)?;"),
                    };
                    block.push_block(block_for);
                    block.line("Ok(())");
                }
                {
                    let mut block = Self::new_read_fn(serializable_implementation);
                    block.line("let mut me = Self::default();");
                    block.line("let len = reader.read_length_determinant()?;");
                    let mut block_for = Block::new("for _ in 0..len");
                    match aliased {
                        Role::Boolean => block_for.line("me.values.push(reader.read_bit()?);"),
                        Role::Integer(_) => block_for.line(format!(
                            "me.values.push(reader.read_int((Self::value_min() as i64, Self::value_max() as i64))? as {});",
                            aliased.clone().into_rust().to_string(),
                        )),
                        Role::UnsignedMaxInteger => {
                            block_for.line("me.values.push(reader.read_int_max()?);")
                        }
                        Role::Custom(custom) => block_for
                            .line(format!("me.values.push({}::read_uper(reader)?);", custom)),
                        Role::UTF8String => {
                            block_for.line(format!("me.values.push(reader.read_utf8_string()?);"))
                        }
                    };
                    block.push_block(block_for);
                    block.line("Ok(me)");
                }
            }
            Definition::Sequence(_name, fields) => {
                {
                    let block = Self::new_write_fn(serializable_implementation);

                    // bitmask for optional fields
                    for field in fields.iter() {
                        if field.optional {
                            block.line(format!(
                                "writer.write_bit(self.{}.is_some())?;",
                                Generator::rust_field_name(&field.name, true),
                            ));
                        }
                    }

                    for field in fields.iter() {
                        let line = match field.role {
                            Role::Boolean => format!(
                                "writer.write_bit({}{})?;",
                                if field.optional { "*" } else { "self." },
                                Generator::rust_field_name(&field.name, true),
                            ),
                            Role::Integer(_) => format!(
                                "writer.write_int({}{} as i64, (Self::{}_min() as i64, Self::{}_max() as i64))?;",
                                if field.optional { "*" } else { "self." },
                                Generator::rust_field_name(&field.name, true),
                                Generator::rust_field_name(&field.name, false),
                                Generator::rust_field_name(&field.name, false),
                            ),
                            Role::UnsignedMaxInteger => format!(
                                "writer.write_int_max({}{})?;",
                                if field.optional { "*" } else { "self." },
                                Generator::rust_field_name(&field.name, true),
                            ),
                            Role::Custom(ref _type) => format!(
                                "{}{}.write_uper(writer)?;",
                                if field.optional { "" } else { "self." },
                                Generator::rust_field_name(&field.name, true),
                            ),
                            Role::UTF8String => format!(
                                "writer.write_utf8_string(&{}{})?;",
                                if field.optional { "" } else { "self." },
                                Generator::rust_field_name(&field.name, true),
                            ),
                        };
                        if field.optional {
                            let mut b = Block::new(&format!(
                                "if let Some(ref {}) = self.{}",
                                Generator::rust_field_name(&field.name, true),
                                Generator::rust_field_name(&field.name, true),
                            ));
                            b.line(line);
                            block.push_block(b);
                        } else {
                            block.line(line);
                        }
                    }

                    block.line("Ok(())");
                }
                {
                    let block = Self::new_read_fn(serializable_implementation);
                    block.line("let mut me = Self::default();");

                    // bitmask for optional fields
                    for field in fields.iter() {
                        if field.optional {
                            block.line(format!(
                                "let {} = reader.read_bit()?;",
                                Generator::rust_field_name(&field.name, true),
                            ));
                        }
                    }
                    for field in fields.iter() {
                        let line = match field.role {
                            Role::Boolean => format!(
                                "me.{} = {}reader.read_bit()?{};",
                                Generator::rust_field_name(&field.name, true),
                                if field.optional { "Some(" } else { "" },
                                if field.optional { ")" } else { "" },
                            ),
                            Role::Integer(_) => format!(
                                "me.{} = {}reader.read_int((Self::{}_min() as i64, Self::{}_max() as i64))? as {}{};",
                                Generator::rust_field_name(&field.name, true),
                                if field.optional { "Some(" } else { "" },
                                Generator::rust_field_name(&field.name, false),
                                Generator::rust_field_name(&field.name, false),
                                field.role.clone().into_rust().to_string(),
                                if field.optional { ")" } else { "" },
                            ),
                            Role::UnsignedMaxInteger => format!(
                                "me.{} = {}reader.read_int_max()?{};",
                                Generator::rust_field_name(&field.name, true),
                                if field.optional { "Some(" } else { "" },
                                if field.optional { ")" } else { "" },
                            ),
                            Role::Custom(ref _type) => format!(
                                "me.{} = {}{}::read_uper(reader)?{};",
                                Generator::rust_field_name(&field.name, true),
                                if field.optional { "Some(" } else { "" },
                                field.role.clone().into_rust().to_string(),
                                if field.optional { ")" } else { "" },
                            ),
                            Role::UTF8String => format!(
                                "me.{} = reader.read_utf8_string()?;",
                                Generator::rust_field_name(&field.name, true),
                            ),
                        };
                        if field.optional {
                            let mut block_if = Block::new(&format!(
                                "if {}",
                                Generator::rust_field_name(&field.name, true),
                            ));
                            block_if.line(line);
                            let mut block_else = Block::new("else");
                            block_else.line(format!(
                                "me.{} = None;",
                                Generator::rust_field_name(&field.name, true),
                            ));
                            block.push_block(block_if);
                            block.push_block(block_else);
                        } else {
                            block.line(line);
                        }
                    }

                    block.line("Ok(me)");
                }
            }
            Definition::Enumeration(name, variants) => {
                {
                    let mut block = Block::new("match self");
                    for (i, variant) in variants.iter().enumerate() {
                        block.line(format!(
                            "{}::{} => writer.write_int({}, (0, {}))?,",
                            name,
                            Generator::rust_variant_name(&variant),
                            i,
                            variants.len() - 1
                        ));
                    }
                    Self::new_write_fn(serializable_implementation)
                        .push_block(block)
                        .line("Ok(())");
                }
                {
                    let mut block = Self::new_read_fn(serializable_implementation);
                    block.line(format!(
                        "let id = reader.read_int((0, {}))?;",
                        variants.len() - 1
                    ));
                    let mut block_match = Block::new("match id");
                    for (i, variant) in variants.iter().enumerate() {
                        block_match.line(format!(
                            "{} => Ok({}::{}),",
                            i,
                            name,
                            Generator::rust_variant_name(&variant),
                        ));
                    }
                    block_match.line(format!(
                        "_ => Err(UperError::ValueNotInRange(id, 0, {}))",
                        variants.len()
                    ));
                    block.push_block(block_match);
                }
            }
        }
    }
}

pub struct ProtobufGenerator;
impl SerializableGenerator for ProtobufGenerator {
    fn add_imports(&self, scope: &mut Scope) {
        Self::add_imports(scope)
    }

    fn generate_implementations(&self, scope: &mut Scope, impl_for: &str, definition: &Definition) {
        Self::impl_eq_fn(
            Self::new_eq_fn(Self::new_eq_impl(scope, impl_for)),
            definition,
        );

        let serializable_impl = Self::new_protobuf_serializable_impl(scope, impl_for);

        Self::impl_format_fn(Self::new_format_fn(serializable_impl), definition);
        Self::impl_write_fn(Self::new_write_fn(serializable_impl), definition);
        Self::impl_read_fn(Self::new_read_fn(serializable_impl), definition);
    }
}

impl ProtobufGenerator {
    const CODEC: &'static str = "Protobuf";

    fn new_protobuf_serializable_impl<'a>(scope: &'a mut Scope, impl_for: &str) -> &'a mut Impl {
        Generator::new_serializable_impl(scope, impl_for, Self::CODEC)
    }

    fn new_read_fn<'a>(implementation: &'a mut Impl) -> &'a mut Function {
        Generator::new_read_fn(implementation, Self::CODEC)
    }

    fn impl_read_fn(function: &mut Function, definition: &Definition) {
        match definition {
            Definition::SequenceOf(name, aliased) => {
                Self::impl_read_fn_for_sequence_of(function, name, aliased);
            }
            Definition::Sequence(name, fields) => {
                Self::impl_read_fn_for_sequence(function, name, &fields[..]);
            }
            Definition::Enumeration(name, variants) => {
                Self::impl_read_fn_for_enumeration(function, name, &variants[..]);
            }
        };
    }

    fn impl_read_fn_for_sequence_of(function: &mut Function, name: &String, aliased: &Role) {
        function.line("let mut me = Self::default();");

        let mut block_while = Block::new("while let Ok(tag) = reader.read_tag()");
        block_while.line(format!(
            "if tag.0 != 1 {{ return Err({}Error::invalid_tag_received(tag.0)); }}",
            Self::CODEC
        ));
        block_while.line(format!("if tag.1 != {}Format::LengthDelimited {{ return Err({}Error::unexpected_format(tag.1)); }}", Self::CODEC, Self::CODEC));
        block_while.line("let bytes = reader.read_bytes()?;");
        let mut block_reader = Block::new("");
        block_reader.line(format!(
            "let reader = &mut &bytes[..] as &mut {}Reader;",
            Self::CODEC
        ));
        match aliased {
            Role::Custom(custom) => block_reader.line(format!(
                "me.values.push({}::read_protobuf(reader)?);",
                custom
            )),
            r => block_reader.line(format!(
                "me.values.push(reader.read_{}()?{});",
                r.clone().into_protobuf().to_string(),
                Self::get_as_rust_type_statement(r),
            )),
        };
        block_while.push_block(block_reader);
        function.push_block(block_while);
        function.line("Ok(me)");
    }

    fn impl_read_fn_for_sequence(function: &mut Function, name: &String, fields: &[Field]) {
        for field in fields.iter() {
            function.line(format!(
                "let mut read_{} = None;",
                Generator::rust_field_name(&field.name, false)
            ));
        }

        let mut block_reader_loop = Block::new("while let Ok(tag) = reader.read_tag()");
        let mut block_match_tag = Block::new("match tag.0");
        block_match_tag.line("0 => break,");

        for (prev_tag, field) in fields.iter().enumerate() {
            match &field.role {
                Role::Custom(name) => {
                    let mut block_case = Block::new(&format!(
                        "{} => read_{} = Some(",
                        prev_tag + 1,
                        Generator::rust_field_name(&field.name, false)
                    ));
                    let mut block_case_if = Block::new(&format!(
                        "if {}::{}_format() == {}Format::LengthDelimited",
                        name,
                        Self::CODEC.to_lowercase(),
                        Self::CODEC
                    ));
                    block_case_if.line("let bytes = reader.read_bytes()?;");
                    block_case_if.line(format!(
                        "{}::read_protobuf(&mut &bytes[..] as &mut {}Reader)?",
                        name,
                        Self::CODEC
                    ));
                    let mut block_case_el = Block::new("else");
                    block_case_el.line(format!("{}::read_protobuf(reader)?", name));
                    block_case.push_block(block_case_if);
                    block_case.push_block(block_case_el);
                    block_case.after("),");
                    block_match_tag.push_block(block_case);
                }
                role => {
                    block_match_tag.line(format!(
                        "{} => read_{} = Some({}),",
                        prev_tag + 1,
                        Generator::rust_field_name(&field.name, false),
                        format!(
                            "reader.read_{}()?",
                            role.clone().into_protobuf().to_string(),
                        )
                    ));
                }
            }
        }

        block_match_tag.line(format!(
            "_ => return Err({}Error::invalid_tag_received(tag.0)),",
            Self::CODEC
        ));
        block_reader_loop.push_block(block_match_tag);
        function.push_block(block_reader_loop);
        let mut return_block = Block::new(&format!("Ok({}", name));
        for field in fields.iter() {
            return_block.line(&format!(
                "{}: read_{}.map(|v| v{}){},",
                Generator::rust_field_name(&field.name, true),
                Generator::rust_field_name(&field.name, false),
                Self::get_as_rust_type_statement(&field.role),
                if field.optional {
                    "".into()
                } else {
                    format!(
                        ".unwrap_or({}::default())",
                        field.role.clone().into_rust().to_string()
                    )
                },
            ));
        }

        return_block.after(")");
        function.push_block(return_block);
    }

    fn impl_read_fn_for_enumeration(function: &mut Function, name: &String, variants: &[String]) {
        let mut block_match = Block::new("match reader.read_varint()?");
        for (field, variant) in variants.iter().enumerate() {
            block_match.line(format!(
                "{} => Ok({}::{}),",
                field,
                name,
                Generator::rust_variant_name(&variant),
            ));
        }
        block_match.line(format!(
            "v => Err({}Error::invalid_variant(v as u32))",
            Self::CODEC,
        ));
        function.push_block(block_match);
    }

    fn new_write_fn<'a>(implementation: &'a mut Impl) -> &'a mut Function {
        Generator::new_write_fn(implementation, Self::CODEC)
    }

    fn impl_write_fn(function: &mut Function, definition: &Definition) {
        match definition {
            Definition::SequenceOf(name, aliased) => {
                Self::impl_write_fn_for_sequence_of(function, name, aliased);
            }
            Definition::Sequence(name, fields) => {
                Self::impl_write_fn_for_sequence(function, name, &fields[..]);
            }
            Definition::Enumeration(name, variants) => {
                Self::impl_write_fn_for_enumeration(function, name, &variants[..]);
            }
        };
        function.line("Ok(())");
    }

    fn impl_write_fn_for_sequence_of(function: &mut Function, name: &String, aliased: &Role) {
        let mut block_writer = Block::new("");
        let mut block_for = Block::new("for value in self.values.iter()");
        block_for.line(format!(
            "writer.write_tag(1, {})?;",
            Self::role_to_format(aliased),
        ));
        block_for.line("let mut bytes = Vec::new();");
        match aliased {
            Role::Custom(_custom) => {
                block_for.line(format!(
                    "value.write_protobuf(&mut bytes as &mut {}Writer)?;",
                    Self::CODEC
                ));
            }
            r => {
                block_for.line(format!(
                    "(&mut bytes as &mut {}Writer).write_{}(*value{})?;",
                    Self::CODEC,
                    r.clone().into_protobuf().to_string(),
                    Self::get_as_protobuf_type_statement(r),
                ));
            }
        };
        block_for.line("writer.write_bytes(&bytes[..])?;");
        block_writer.push_block(block_for);
        function.push_block(block_writer);
    }

    fn impl_write_fn_for_sequence(function: &mut Function, name: &String, fields: &[Field]) {
        for (prev_tag, field) in fields.iter().enumerate() {
            let block_: &mut Function = function;
            let mut block = if field.optional {
                Block::new(&format!(
                    "if let Some(ref {}) = self.{}",
                    Generator::rust_field_name(&field.name, true),
                    Generator::rust_field_name(&field.name, true),
                ))
            } else {
                Block::new("")
            };

            match &field.role {
                Role::Custom(_custom) => {
                    let format_line =
                        format!("{}::{}_format()", _custom, Self::CODEC.to_lowercase());
                    block.line(format!(
                        "writer.write_tag({}, {})?;",
                        prev_tag + 1,
                        format_line,
                    ));
                    let mut block_if = Block::new(&format!(
                        "if {} == {}Format::LengthDelimited",
                        format_line,
                        Self::CODEC
                    ));
                    block_if.line("let mut vec = Vec::new();");
                    block_if.line(format!(
                        "{}{}.write_protobuf(&mut vec as &mut {}Writer)?;",
                        if field.optional { "" } else { "self." },
                        Generator::rust_field_name(&field.name, true),
                        Self::CODEC,
                    ));
                    block_if.line("writer.write_bytes(&vec[..])?;");

                    let mut block_el = Block::new("else");
                    block_el.line(format!(
                        "{}{}.write_protobuf(writer)?;",
                        if field.optional { "" } else { "self." },
                        Generator::rust_field_name(&field.name, true),
                    ));

                    block.push_block(block_if);
                    block.push_block(block_el);
                }
                r => {
                    block.line(format!(
                        "writer.write_tagged_{}({}, {}{}{})?;",
                        r.clone().into_protobuf().to_string(),
                        prev_tag + 1,
                        if ProtobufType::String == r.clone().into_protobuf() {
                            if field.optional {
                                ""
                            } else {
                                "&self."
                            }
                        } else {
                            if field.optional {
                                "*"
                            } else {
                                "self."
                            }
                        },
                        Generator::rust_field_name(&field.name, true),
                        Self::get_as_protobuf_type_statement(r),
                    ));
                }
            };
            block_.push_block(block);
        }
    }

    fn impl_write_fn_for_enumeration(function: &mut Function, name: &String, variants: &[String]) {
        let mut outer_block = Block::new("match self");
        for (field, variant) in variants.iter().enumerate() {
            outer_block.line(format!(
                "{}::{} => writer.write_varint({})?,",
                name,
                Generator::rust_variant_name(&variant),
                field,
            ));
        }
        function.push_block(outer_block);
    }

    fn new_format_fn<'a>(implementation: &'a mut Impl) -> &'a mut Function {
        implementation
            .new_fn(&format!("{}_format", Self::CODEC.to_lowercase()))
            .ret(format!("{}Format", Self::CODEC))
    }

    fn impl_format_fn(function: &mut Function, definition: &Definition) {
        let format = match definition {
            Definition::SequenceOf(_, _) => ProtobufFormat::LengthDelimited,
            Definition::Sequence(_, _) => ProtobufFormat::LengthDelimited,
            Definition::Enumeration(_, _) => ProtobufFormat::VarInt,
        };
        function.line(format!("{}Format::{}", Self::CODEC, format.to_string()));
    }

    fn new_eq_impl<'a>(scope: &'a mut Scope, name: &str) -> &'a mut Impl {
        scope
            .new_impl(name)
            .impl_trait(&format!("{}Eq", Self::CODEC))
    }

    fn new_eq_fn<'a>(implementation: &'a mut Impl) -> &'a mut Function {
        implementation
            .new_fn(&format!("{}_eq", Self::CODEC.to_lowercase()))
            .ret("bool")
            .arg_ref_self()
            .arg("other", format!("&Self"))
    }

    fn impl_eq_fn(function: &mut Function, definition: &Definition) {
        match definition {
            Definition::SequenceOf(_, _) => {
                function.line(format!(
                    "self.values.{}_eq(&other.values)",
                    Self::CODEC.to_lowercase()
                ));
            }
            Definition::Sequence(_, fields) => {
                for (num, field) in fields.iter().enumerate() {
                    if num > 0 {
                        function.line("&&");
                    }
                    let field_name = Generator::rust_field_name(&field.name, true);
                    function.line(&format!(
                        "self.{}.{}_eq(&other.{})",
                        field_name,
                        Self::CODEC.to_lowercase(),
                        field_name
                    ));
                }
            }
            Definition::Enumeration(_, _) => {
                function.line("self == other");
            }
        }
    }

    fn add_imports(scope: &mut Scope) {
        scope.import("asn1c::io::protobuf", Self::CODEC);
        scope.import(
            "asn1c::io::protobuf",
            &format!("ProtobufEq as {}Eq", Self::CODEC),
        );
        scope.import(
            "asn1c::io::protobuf",
            &format!("Reader as {}Reader", Self::CODEC),
        );
        scope.import(
            "asn1c::io::protobuf",
            &format!("Writer as {}Writer", Self::CODEC),
        );
        scope.import(
            "asn1c::io::protobuf",
            &format!("Error as {}Error", Self::CODEC),
        );
        scope.import(
            "asn1c::io::protobuf",
            &format!("Format as {}Format", Self::CODEC),
        );
    }

    fn role_to_format(role: &Role) -> String {
        match role.clone().into_protobuf() {
            ProtobufType::Bool => format!("{}Format::VarInt", Self::CODEC),
            ProtobufType::SFixed32 => format!("{}Format::Fixed32", Self::CODEC),
            ProtobufType::SFixed64 => format!("{}Format::Fixed64", Self::CODEC),
            ProtobufType::UInt32 => format!("{}Format::VarInt", Self::CODEC),
            ProtobufType::UInt64 => format!("{}Format::VarInt", Self::CODEC),
            ProtobufType::SInt32 => format!("{}Format::VarInt", Self::CODEC),
            ProtobufType::SInt64 => format!("{}Format::VarInt", Self::CODEC),
            ProtobufType::String => format!("{}Format::LengthDelimited", Self::CODEC),
            ProtobufType::Complex(complex) => {
                format!("{}::{}_format()", complex, Self::CODEC.to_lowercase())
            }
        }
    }

    fn get_as_protobuf_type_statement(role: &Role) -> String {
        let role_rust = role.clone().into_rust();
        let proto_rust = role.clone().into_protobuf().into_rust();

        if role_rust != proto_rust {
            format!(" as {}", proto_rust.to_string())
        } else {
            "".into()
        }
    }

    fn get_as_rust_type_statement(role: &Role) -> String {
        let role_rust = role.clone().into_rust();
        let proto_rust = role.clone().into_protobuf().into_rust();

        if role_rust != proto_rust {
            format!(" as {}", role_rust.to_string())
        } else {
            "".into()
        }
    }
}
