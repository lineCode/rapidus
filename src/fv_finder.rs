use node::{Node, NodeBase, PropertyDefinition};

use rand::random;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct FreeVariableFinder {
    pub varmap: Vec<HashSet<String>>,
    pub cur_fv: Vec<HashSet<String>>,
    pub mangled_function_name: Vec<HashMap<String, String>>,
    pub use_this: bool,
}

impl FreeVariableFinder {
    pub fn new() -> FreeVariableFinder {
        let mut varmap = HashSet::new(); // global
        varmap.insert("console".to_string());
        varmap.insert("process".to_string());
        FreeVariableFinder {
            varmap: vec![varmap],
            cur_fv: vec![HashSet::new()],
            mangled_function_name: vec![],
            use_this: false,
        }
    }

    pub fn run_toplevel(&mut self, node: &mut Node) {
        match &mut node.base {
            &mut NodeBase::StatementList(ref mut nodes) => {
                let mut func_decl_index = vec![];
                self.varmap.push(HashSet::new()); // main ( local )

                for (i, node) in nodes.iter_mut().enumerate() {
                    match node.base {
                        NodeBase::FunctionDecl(ref name, _, _, _, _) => {
                            self.varmap[0].insert(name.clone());
                            func_decl_index.push(i)
                        }
                        _ => self.run(node),
                    }
                }

                for index in func_decl_index {
                    self.run(&mut nodes[index]);
                    self.use_this = false;
                }
            }
            _ => unreachable!(),
        }
    }

    fn run(&mut self, node: &mut Node) {
        match &mut node.base {
            &mut NodeBase::StatementList(ref mut nodes) => {
                for node in nodes {
                    self.run(node)
                }
            }
            &mut NodeBase::FunctionDecl(
                ref mut name,
                ref mut use_this,
                ref mut fv,
                ref params,
                ref mut body,
            ) => {
                self.varmap.push(HashSet::new());
                self.varmap.last_mut().unwrap().insert(name.clone());

                for param in params.clone() {
                    self.varmap.last_mut().unwrap().insert(param.name);
                }

                let mut body = if let &mut NodeBase::StatementList(ref mut body) = &mut body.base
                {
                    body
                } else {
                    unreachable!()
                };

                self.mangled_function_name.push(HashMap::new());

                let mut func_decl_index = vec![];
                for (i, node) in body.iter_mut().enumerate() {
                    match &mut node.base {
                        &mut NodeBase::FunctionDecl(ref mut name, _, _, _, _) => {
                            let nested = self.varmap.len() + 1 > 3;
                            let mangled_name = if nested {
                                Some(format!("{}.{}", name.clone(), random::<u32>()))
                            } else {
                                None
                            };

                            self.varmap.last_mut().unwrap().insert(name.clone());

                            if let Some(ref mangled_name) = mangled_name {
                                self.mangled_function_name
                                    .last_mut()
                                    .unwrap()
                                    .insert(name.clone(), mangled_name.clone());
                            }

                            if nested {
                                *name = mangled_name.clone().unwrap();
                            }

                            func_decl_index.push(i)
                        }
                        _ => {}
                    }
                }

                self.cur_fv.push(HashSet::new());

                for node in body.iter_mut() {
                    match &node.base {
                        &NodeBase::FunctionDecl(_, _, _, _, _) => {}
                        _ => self.run(node),
                    }
                }

                for index in func_decl_index {
                    self.run(&mut body[index])
                }

                self.mangled_function_name.pop();

                *use_this = self.use_this;

                self.varmap.pop();

                self.varmap.last_mut().unwrap().insert(name.clone());

                let fv_ = self.cur_fv.pop().unwrap();
                *fv = fv_.clone();
                for name in fv_ {
                    self.cur_fv.last_mut().unwrap().insert(name);
                }
                for name in self.varmap.last().unwrap().iter() {
                    self.cur_fv.last_mut().unwrap().remove(name);
                }
            }
            &mut NodeBase::Call(ref mut callee, ref mut args) => {
                self.run(callee);
                for arg in args {
                    self.run(arg)
                }
            }
            &mut NodeBase::VarDecl(ref name, ref mut init) => {
                self.varmap.last_mut().unwrap().insert(name.clone());
                if let &mut Some(ref mut init) = init {
                    self.run(init)
                }
            }
            &mut NodeBase::Return(ref mut val) => {
                if let &mut Some(ref mut val) = val {
                    self.run(&mut **val)
                }
            }
            &mut NodeBase::Member(ref mut parent, _) => {
                self.run(&mut *parent);
            }
            &mut NodeBase::Index(ref mut parent, ref mut idx) => {
                self.run(&mut *parent);
                self.run(&mut *idx);
            }
            &mut NodeBase::This => self.use_this = true,
            &mut NodeBase::Identifier(ref mut name) => self.identifier(name),
            &mut NodeBase::Object(ref mut properties) => {
                for property in properties.iter_mut() {
                    let name_of_ident_ref =
                        if let PropertyDefinition::IdentifierReference(name) = property.clone() {
                            Some(name)
                        } else {
                            None
                        };
                    match property {
                        &mut PropertyDefinition::IdentifierReference(_) => {
                            let mut name_of_ident_ref = name_of_ident_ref.unwrap();
                            *property = PropertyDefinition::Property(
                                name_of_ident_ref.to_string(),
                                Node::new(
                                    NodeBase::Identifier({
                                        self.identifier(&mut name_of_ident_ref);
                                        name_of_ident_ref
                                    }),
                                    node.pos, // TODO: Is this correct?
                                ),
                            );
                        }
                        &mut PropertyDefinition::Property(_, ref mut node) => self.run(node),
                    }
                }
            }
            &mut NodeBase::If(ref mut cond, ref mut then, ref mut else_) => {
                self.run(&mut *cond);
                self.run(&mut *then);
                self.run(&mut *else_);
            }
            &mut NodeBase::While(ref mut cond, ref mut body) => {
                self.run(&mut *cond);
                self.run(&mut *body);
            }
            &mut NodeBase::Assign(ref mut dst, ref mut src) => {
                match &mut dst.base {
                    &mut NodeBase::Identifier(ref name) => {
                        if !self.varmap.iter().any(|v| v.contains(name.as_str())) {
                            // If such a variable didn't appear before, this assignment
                            // serves the declaration of it as a global variable.
                            self.varmap[0].insert(name.clone());
                        } else if !self.varmap[0].contains(name.as_str())
                            && !self.varmap.last().unwrap().contains(name.as_str())
                        {
                            self.cur_fv.last_mut().unwrap().insert(name.clone());
                        }
                    }
                    &mut NodeBase::Member(ref mut parent, _) => {
                        self.run(parent);
                    }
                    &mut NodeBase::Index(ref mut parent, ref mut idx) => {
                        self.run(parent);
                        self.run(idx);
                    }
                    _ => unimplemented!(),
                }
                self.run(&mut *src);
            }
            &mut NodeBase::UnaryOp(ref mut expr, _) => {
                self.run(&mut *expr);
            }
            &mut NodeBase::BinaryOp(ref mut lhs, ref mut rhs, _) => {
                self.run(&mut *lhs);
                self.run(&mut *rhs);
            }
            &mut NodeBase::TernaryOp(ref mut cond, ref mut then, ref mut else_) => {
                self.run(&mut *cond);
                self.run(&mut *then);
                self.run(&mut *else_);
            }
            _ => {}
        }
    }

    fn identifier(&mut self, name: &mut String) {
        let is_cur_scope_var = self.varmap.last().unwrap().contains(name.as_str());
        let varmap_len = self.varmap.len();
        let is_already_appeared_var_but_not_in_cur_scope_or_global = self.varmap[1..varmap_len - 1]
            .iter()
            .any(|v| v.contains(name.as_str()));

        for mangled_function_name in self.mangled_function_name.iter().rev() {
            if let Some(mangled_name) = mangled_function_name.get(name.as_str()) {
                *name = mangled_name.clone();
                break;
            }
        }

        if !is_cur_scope_var && is_already_appeared_var_but_not_in_cur_scope_or_global {
            self.cur_fv.last_mut().unwrap().insert(name.clone());
        }
    }
}
