extern crate regex;

extern crate clang;

#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;

extern crate arguments;

fn count_lines(range: clang::source::SourceRange) -> u32 {
    let (_file, line_s, _col) = range.get_start().get_presumed_location();
    let (_file, line_e, _col) = range.get_end().get_presumed_location();

    line_e - line_s + 1
}

fn is_statement(stmt: &clang::Entity) -> bool {
    stmt.is_statement() || stmt.is_expression()
}

fn is_conditional(stmt: &clang::Entity) -> bool {
    //println!("{:?}", stmt);
    assert!(is_statement(stmt));
    match stmt.get_kind() {
        clang::EntityKind::IfStmt => true,
        clang::EntityKind::WhileStmt => true,
        clang::EntityKind::DoStmt => true,
        clang::EntityKind::ForStmt => true,
        clang::EntityKind::SwitchStmt => true,
        _ => false,
    }
}

fn get_body<'tu>(stmt: &clang::Entity<'tu>) -> (clang::Entity<'tu>, Option<clang::Entity<'tu>>) {
    if stmt.get_kind() == clang::EntityKind::IfStmt {
        let children = stmt.get_children();
        if children.len() == 2 {
            let body = children.last().unwrap();
            (*body, None)
        } else {
            let body = children[1];
            let body_else = children[2];
            (body, Some(body_else))
        }
    } else {
        let children = stmt.get_children();
        let body = children.last().unwrap();
        (*body, None)
    }
}

fn process_conditional(stmt: &clang::Entity) -> (u32, u32) {
    // get body
    let (body, body_else) = get_body(&stmt);
    let (mut nodes, mut edges) = cyclomatic_complexity(&body);

    //if we have an else clause, we add an edge and acount the block
    if let Some(body) = body_else {
        let (n, e) = cyclomatic_complexity(&body);

        nodes += n;
        edges += e + 1;
    }

    (nodes, edges)
}

fn cyclomatic_complexity(stmt: &clang::Entity) -> (u32, u32) {
    if stmt.get_kind() == clang::EntityKind::CompoundStmt {
        // for each statement:
        stmt.get_children()
            .iter()
            .fold((0, 0), |(nodes, edges), stmt| {
                let mut nodes = nodes + 1;
                let mut edges = edges + 1;

                if is_conditional(stmt) {
                    if stmt.get_kind() != clang::EntityKind::SwitchStmt {
                        edges += 1;
                    }

                    let (n, e) = process_conditional(stmt);
                    nodes += n;
                    edges += e;
                }

                // we may be inside of a switch, count the number of cases and defaults as edges
                if stmt.get_kind() == clang::EntityKind::CaseStmt
                    || stmt.get_kind() == clang::EntityKind::DefaultStmt
                {
                    edges += 1;
                }

                (nodes, edges)
            })
    } else {
        if is_conditional(stmt) {
            process_conditional(stmt)
        } else {
            (1, 1)
        }
    }
}

fn get_file_location(node: &clang::Entity) -> Result<String, ()> {
    let loc = node.get_location().unwrap();
    let loc = loc.get_spelling_location().file.unwrap().get_path();
    Ok(loc.to_str().unwrap().to_string())
}

fn get_filename(path: &String) -> Option<&str> {
    use std::path;
    let path = path::Path::new(path.as_str());
    path.file_name().map(|osstr| osstr.to_str()).unwrap()
}

fn get_module(path: &String) -> Option<&str> {
    use std::path;
    let path = path::Path::new(path.as_str());
    path.parent().unwrap().to_str()
}

#[derive(Clone, Debug)]
struct ProcessCtx {
    namespaces: String,
    header_regex: Option<regex::Regex>,
}

impl ProcessCtx {
    fn process_fn(&self, fun: &clang::Entity) {
        if let None = fun.get_child(0) {
            // just a declaration, no body
            return;
        }

        let loc = get_file_location(fun).unwrap_or("no_location".to_string());
        let file = get_filename(&loc).unwrap();
        let module = get_module(&loc).unwrap();
        let name = self.get_qualified_name(fun);
        let lines = count_lines(fun.get_range().unwrap());

        let arg_count = fun.get_arguments().unwrap().len();

        let body = fun.get_children().last().unwrap().clone();
        if body.get_kind() != clang::EntityKind::CompoundStmt {
            return;
        }
        let (n, e) = cyclomatic_complexity(&body);
        let comp = e - n; // using simplied formula, avoid (+ 2p);

        println!(
            "{:60}\t{:20}\t{:80}\t{}\t{}\t{}",
            module, file, name, arg_count, lines, comp
        );
    }

    fn process_named_nested(&self, node: &clang::Entity) {
        let mut new_ctx = self.clone();
        new_ctx.push_name(node.get_name().unwrap_or("anonymous".to_string()));
        new_ctx.process_node(&node);
    }

    fn process_node(&self, node: &clang::Entity) {
        node.visit_children(
            |node: clang::Entity, _parent: clang::Entity| -> clang::EntityVisitResult {
                let x = node.get_location();
                if let Some(loc) = x {
                    match (&self.header_regex, get_file_location(&node)) {
                        (Some(ref r), Ok(ref path)) => {
                            if !r.is_match(path.as_str()) {
                                return clang::EntityVisitResult::Continue;
                            }
                        }
                        (None, _) => {
                            if !loc.is_in_main_file() {
                                return clang::EntityVisitResult::Continue;
                            }
                        }
                        (_, _) => {}
                    }
                } else {
                    return clang::EntityVisitResult::Continue;
                }

                match node.get_kind() {
                    clang::EntityKind::Namespace => {
                        self.process_named_nested(&node);
                        clang::EntityVisitResult::Continue
                    }
                    clang::EntityKind::ClassDecl => {
                        self.process_named_nested(&node);
                        clang::EntityVisitResult::Continue
                    }
                    clang::EntityKind::UnionDecl => {
                        self.process_named_nested(&node);
                        clang::EntityVisitResult::Continue
                    }
                    clang::EntityKind::StructDecl => {
                        self.process_named_nested(&node);
                        clang::EntityVisitResult::Continue
                    }
                    clang::EntityKind::FunctionDecl => {
                        self.process_fn(&node);
                        clang::EntityVisitResult::Continue
                    }
                    clang::EntityKind::Method => {
                        self.process_fn(&node);
                        clang::EntityVisitResult::Continue
                    }
                    _ => clang::EntityVisitResult::Continue,
                }
            },
        );
    }

    fn push_name(&mut self, name: String) {
        if self.namespaces.is_empty() {
            self.namespaces = name;
        } else {
            self.namespaces = format!("{}::{}", self.namespaces, name);
        }
    }

    fn get_qualified_name(&self, node: &clang::Entity) -> String {
        let mut name = node.get_name().unwrap();

        if node.get_kind() == clang::EntityKind::Method {
            for c in node.get_children() {
                if c.is_reference() {
                    let r = c.get_reference().unwrap();
                    let class_name = r.get_name().unwrap();
                    name = format!("{}::{}", class_name, name)
                }
            }
        }

        if self.namespaces.is_empty() {
            return name;
        }

        format!("{}::{}", self.namespaces, name)
    }
}

fn print_header() {
    let module = "module";
    let file = "file";
    let name = "name";
    let arg_count = "args";
    let lines = "lines";
    let comp = "McCabe";
    println!(
        "{:60}\t{:20}\t{:80}\t{}\t{}\t{}",
        module, file, name, arg_count, lines, comp
    );
}

fn process_file(file: &str, args: &[&str], hr: Option<String>) {
    let clang = clang::Clang::new().unwrap();
    let index = clang::Index::new(&clang, false, false);

    let tu = index
        .parser(file)
        .arguments(args)
        .parse()
        .expect("should parse");

    let header_rex = hr.map(|s| regex::Regex::new(s.as_str()).unwrap());
    let ctx = ProcessCtx {
        namespaces: String::new(),
        header_regex: header_rex,
    };

    ctx.process_node(&tu.get_entity());
}

#[derive(Serialize, Deserialize, Debug)]
struct CompilationEntry {
    pub directory: String,
    pub command: String,
    pub file: String,
}

fn main() {
    eprintln!("using clang {}", clang::get_version());
    let args = std::env::args();
    let arguments = arguments::parse(args).unwrap();

    let headers = arguments.get::<String>("headers");

    if let Some(conf) = arguments.get::<String>("conf") {
        use std::fs::File;
        use std::io::BufReader;

        let file = File::open(conf).unwrap();
        let reader = BufReader::new(file);
        let comp_db: Vec<CompilationEntry> = serde_json::from_reader(reader).unwrap();

        print_header();

        for cfg in comp_db {
            let mut comp_args: Vec<&str> =
                cfg.command.split(" ").filter(|s| !s.is_empty()).collect();
            comp_args.remove(0);

            let len = comp_args.len();
            process_file(
                cfg.file.as_str(),
                &comp_args.as_slice()[0..len - 4],
                headers.clone(),
            );
        }
    }

    if let Some(file) = arguments.get::<String>("file") {
        print_header();
        process_file(file.as_str(), &[], headers.clone());
    }
}
