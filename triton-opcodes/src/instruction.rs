use std::collections::HashMap;
use std::collections::HashSet;
use std::error::Error;
use std::fmt::Display;
use std::ops::Neg;
use std::str::SplitWhitespace;
use std::vec;

use anyhow::bail;
use anyhow::Result;
use itertools::Itertools;
use num_traits::One;
use regex::Regex;
use strum::EnumCount;
use strum::IntoEnumIterator;
use strum_macros::Display as DisplayMacro;
use strum_macros::EnumCount as EnumCountMacro;
use strum_macros::EnumIter;

use twenty_first::shared_math::b_field_element::BFieldElement;

use AnInstruction::*;
use TokenError::*;

use crate::instruction::DivinationHint::Quotient;
use crate::ord_n::Ord16;
use crate::ord_n::Ord16::*;
use crate::ord_n::Ord7;

/// An `Instruction` has `call` addresses encoded as absolute integers.
pub type Instruction = AnInstruction<BFieldElement>;

/// A `LabelledInstruction` has `call` addresses encoded as label names.
///
/// A label name is a `String` that occurs as "`label_name`:".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LabelledInstruction {
    Instruction(AnInstruction<String>),
    Label(String),
}

impl Display for LabelledInstruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LabelledInstruction::Instruction(instr) => write!(f, "{}", instr),
            LabelledInstruction::Label(label_name) => write!(f, "{}:", label_name),
        }
    }
}

#[derive(Debug, DisplayMacro, Clone, Copy, PartialEq, Eq, Hash, EnumCountMacro)]
pub enum DivinationHint {
    Quotient,
}

/// A Triton VM instruction
///
/// The ISA is defined at:
///
/// https://triton-vm.org/spec/isa.html
///
/// The type parameter `Dest` describes the type of addresses (absolute or labels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumCountMacro, EnumIter)]
pub enum AnInstruction<Dest: PartialEq + Default> {
    // OpStack manipulation
    Pop,
    Push(BFieldElement),
    Divine(Option<DivinationHint>),
    Dup(Ord16),
    Swap(Ord16),

    // Control flow
    Nop,
    Skiz,
    Call(Dest),
    Return,
    Recurse,
    Assert,
    Halt,

    // Memory access
    ReadMem,
    WriteMem,

    // Hashing-related instructions
    Hash,
    DivineSibling,
    AssertVector,

    // Arithmetic on stack instructions
    Add,
    Mul,
    Invert,
    Split,
    Eq,
    Lsb,

    XxAdd,
    XxMul,
    XInvert,
    XbMul,

    // Read/write
    ReadIo,
    WriteIo,
}

impl<Dest: Display + PartialEq + Default> Display for AnInstruction<Dest> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // OpStack manipulation
            Pop => write!(f, "pop"),
            Push(arg) => write!(f, "push {}", arg),
            Divine(Some(hint)) => write!(f, "divine_{}", format!("{hint}").to_ascii_lowercase()),
            Divine(None) => write!(f, "divine"),
            Dup(arg) => write!(f, "dup{}", arg),
            Swap(arg) => write!(f, "swap{}", arg),
            // Control flow
            Nop => write!(f, "nop"),
            Skiz => write!(f, "skiz"),
            Call(arg) => write!(f, "call {}", arg),
            Return => write!(f, "return"),
            Recurse => write!(f, "recurse"),
            Assert => write!(f, "assert"),
            Halt => write!(f, "halt"),

            // Memory access
            ReadMem => write!(f, "read_mem"),
            WriteMem => write!(f, "write_mem"),

            // Hash instructions
            Hash => write!(f, "hash"),
            DivineSibling => write!(f, "divine_sibling"),
            AssertVector => write!(f, "assert_vector"),

            // Arithmetic on stack instructions
            Add => write!(f, "add"),
            Mul => write!(f, "mul"),
            Invert => write!(f, "invert"),
            Split => write!(f, "split"),
            Eq => write!(f, "eq"),
            Lsb => write!(f, "lsb"),

            XxAdd => write!(f, "xxadd"),
            XxMul => write!(f, "xxmul"),
            XInvert => write!(f, "xinvert"),
            XbMul => write!(f, "xbmul"),

            // Read/write
            ReadIo => write!(f, "read_io"),
            WriteIo => write!(f, "write_io"),
        }
    }
}

impl<Dest: PartialEq + Default> AnInstruction<Dest> {
    /// Drop the specific argument in favor of a default one.
    pub fn strip(&self) -> Self {
        match self {
            Push(_) => Push(Default::default()),
            Divine(_) => Divine(Default::default()),
            Dup(_) => Dup(Default::default()),
            Swap(_) => Swap(Default::default()),
            Call(_) => Call(Default::default()),
            Pop => Pop,
            Nop => Nop,
            Skiz => Skiz,
            Return => Return,
            Recurse => Recurse,
            Assert => Assert,
            Halt => Halt,
            ReadMem => ReadMem,
            WriteMem => WriteMem,
            Hash => Hash,
            DivineSibling => DivineSibling,
            AssertVector => AssertVector,
            Add => Add,
            Mul => Mul,
            Invert => Invert,
            Split => Split,
            Eq => Eq,
            Lsb => Lsb,
            XxAdd => XxAdd,
            XxMul => XxMul,
            XInvert => XInvert,
            XbMul => XbMul,
            ReadIo => ReadIo,
            WriteIo => WriteIo,
        }
    }

    /// Assign a unique positive integer to each `Instruction`.
    pub fn opcode(&self) -> u32 {
        match self {
            Pop => 2,
            Push(_) => 1,
            Divine(_) => 4,
            Dup(_) => 5,
            Swap(_) => 9,
            Nop => 8,
            Skiz => 6,
            Call(_) => 13,
            Return => 12,
            Recurse => 16,
            Assert => 10,
            Halt => 0,
            ReadMem => 20,
            WriteMem => 24,
            Hash => 28,
            DivineSibling => 32,
            AssertVector => 36,
            Add => 14,
            Mul => 18,
            Invert => 40,
            Split => 44,
            Eq => 22,
            Lsb => 48,
            XxAdd => 52,
            XxMul => 56,
            XInvert => 60,
            XbMul => 26,
            ReadIo => 64,
            WriteIo => 30,
        }
    }

    /// Returns whether a given instruction modifies the op-stack.
    ///
    /// A modification involves any amount of pushing and/or popping.
    pub fn is_op_stack_instruction(&self) -> bool {
        !matches!(
            self,
            Nop | Call(_) | Return | Recurse | Halt | Hash | AssertVector
        )
    }

    pub fn opcode_b(&self) -> BFieldElement {
        self.opcode().into()
    }

    pub fn size(&self) -> usize {
        if matches!(self, Push(_) | Dup(_) | Swap(_) | Call(_)) {
            2
        } else {
            1
        }
    }

    /// Get the i'th instruction bit
    pub fn ib(&self, arg: Ord7) -> BFieldElement {
        let opcode = self.opcode();
        let bit_number: usize = arg.into();

        ((opcode >> bit_number) & 1).into()
    }

    fn map_call_address<F, NewDest: PartialEq + Default>(&self, f: F) -> AnInstruction<NewDest>
    where
        F: Fn(&Dest) -> NewDest,
    {
        match self {
            Pop => Pop,
            Push(x) => Push(*x),
            Divine(x) => Divine(*x),
            Dup(x) => Dup(*x),
            Swap(x) => Swap(*x),
            Nop => Nop,
            Skiz => Skiz,
            Call(label) => Call(f(label)),
            Return => Return,
            Recurse => Recurse,
            Assert => Assert,
            Halt => Halt,
            ReadMem => ReadMem,
            WriteMem => WriteMem,
            Hash => Hash,
            DivineSibling => DivineSibling,
            AssertVector => AssertVector,
            Add => Add,
            Mul => Mul,
            Invert => Invert,
            Split => Split,
            Eq => Eq,
            Lsb => Lsb,
            XxAdd => XxAdd,
            XxMul => XxMul,
            XInvert => XInvert,
            XbMul => XbMul,
            ReadIo => ReadIo,
            WriteIo => WriteIo,
        }
    }
}

impl Instruction {
    pub fn arg(&self) -> Option<BFieldElement> {
        match self {
            // Double-word instructions (instructions that take arguments)
            Push(arg) => Some(*arg),
            Dup(arg) => Some(ord16_to_bfe(arg)),
            Swap(arg) => Some(ord16_to_bfe(arg)),
            Call(arg) => Some(*arg),
            _ => None,
        }
    }
}

impl TryFrom<u32> for Instruction {
    type Error = anyhow::Error;

    fn try_from(opcode: u32) -> Result<Self> {
        if let Some(instruction) =
            Instruction::iter().find(|instruction| instruction.opcode() == opcode)
        {
            Ok(instruction)
        } else {
            bail!("No instruction with opcode {} exists.", opcode)
        }
    }
}

impl TryFrom<u64> for Instruction {
    type Error = anyhow::Error;

    fn try_from(opcode: u64) -> Result<Self> {
        (opcode as u32).try_into()
    }
}

impl TryFrom<usize> for Instruction {
    type Error = anyhow::Error;

    fn try_from(opcode: usize) -> Result<Self> {
        (opcode as u32).try_into()
    }
}

fn ord16_to_bfe(n: &Ord16) -> BFieldElement {
    let n: u32 = n.into();
    n.into()
}

#[derive(Debug)]
pub enum TokenError {
    UnexpectedEndOfStream,
    UnknownInstruction(String),
}

impl Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnknownInstruction(s) => write!(f, "UnknownInstruction({})", s),
            UnexpectedEndOfStream => write!(f, "UnexpectedEndOfStream"),
        }
    }
}

impl Error for TokenError {}

/// Convert a program with labels to a program with absolute positions
pub fn convert_labels(program: &[LabelledInstruction]) -> Vec<Instruction> {
    let mut label_map = HashMap::<String, usize>::new();
    let mut instruction_pointer: usize = 0;

    // 1. Add all labels to a map
    for labelled_instruction in program.iter() {
        match labelled_instruction {
            LabelledInstruction::Label(label_name) => {
                label_map.insert(label_name.clone(), instruction_pointer);
            }

            LabelledInstruction::Instruction(instr) => {
                instruction_pointer += instr.size();
            }
        }
    }

    // 2. Convert every label to the lookup value of that map
    program
        .iter()
        .flat_map(|labelled_instruction| convert_labels_helper(labelled_instruction, &label_map))
        .collect()
}

fn convert_labels_helper(
    instruction: &LabelledInstruction,
    label_map: &HashMap<String, usize>,
) -> Vec<Instruction> {
    match instruction {
        LabelledInstruction::Label(_) => vec![],

        LabelledInstruction::Instruction(instr) => {
            let unlabelled_instruction: Instruction = instr.map_call_address(|label_name| {
                let label_not_found = format!("Label not found: {}", label_name);
                let absolute_address = label_map.get(label_name).expect(&label_not_found);
                BFieldElement::new(*absolute_address as u64)
            });

            vec![unlabelled_instruction]
        }
    }
}

pub fn parse(code_with_comments: &str) -> Result<Vec<LabelledInstruction>> {
    let remove_comments = Regex::new(r"//.*?(?:\n|$)").expect("a regex that matches comments");
    let code = remove_comments.replace_all(code_with_comments, "");
    let mut tokens = code.split_whitespace();
    let mut instructions = vec![];

    while let Some(token) = tokens.next() {
        let mut instruction = parse_token(token, &mut tokens)?;
        instructions.append(&mut instruction);
    }

    let all_labels: Vec<String> = instructions
        .iter()
        .flat_map(|instr| match instr {
            LabelledInstruction::Instruction(_) => vec![],
            LabelledInstruction::Label(label) => vec![label.clone()],
        })
        .collect();
    let mut seen_labels: HashSet<String> = HashSet::default();
    let mut duplicate_labels: HashSet<String> = HashSet::default();
    for label in all_labels {
        if !seen_labels.insert(label.clone()) {
            duplicate_labels.insert(label);
        }
    }

    if !duplicate_labels.is_empty() {
        bail!("Duplicate labels: {}", duplicate_labels.iter().join(", "));
    }

    Ok(instructions)
}

fn parse_token(token: &str, tokens: &mut SplitWhitespace) -> Result<Vec<LabelledInstruction>> {
    if let Some(label) = token.strip_suffix(':') {
        let label_name = label.to_string();
        return Ok(vec![LabelledInstruction::Label(label_name)]);
    }

    let instruction: Vec<AnInstruction<String>> = match token {
        // OpStack manipulation
        "pop" => vec![Pop],
        "push" => vec![Push(parse_elem(tokens)?)],
        "divine" => vec![Divine(None)],
        "divine_quotient" => vec![Divine(Some(Quotient))],
        "dup0" => vec![Dup(ST0)],
        "dup1" => vec![Dup(ST1)],
        "dup2" => vec![Dup(ST2)],
        "dup3" => vec![Dup(ST3)],
        "dup4" => vec![Dup(ST4)],
        "dup5" => vec![Dup(ST5)],
        "dup6" => vec![Dup(ST6)],
        "dup7" => vec![Dup(ST7)],
        "dup8" => vec![Dup(ST8)],
        "dup9" => vec![Dup(ST9)],
        "dup10" => vec![Dup(ST10)],
        "dup11" => vec![Dup(ST11)],
        "dup12" => vec![Dup(ST12)],
        "dup13" => vec![Dup(ST13)],
        "dup14" => vec![Dup(ST14)],
        "dup15" => vec![Dup(ST15)],
        "swap1" => vec![Swap(ST1)],
        "swap2" => vec![Swap(ST2)],
        "swap3" => vec![Swap(ST3)],
        "swap4" => vec![Swap(ST4)],
        "swap5" => vec![Swap(ST5)],
        "swap6" => vec![Swap(ST6)],
        "swap7" => vec![Swap(ST7)],
        "swap8" => vec![Swap(ST8)],
        "swap9" => vec![Swap(ST9)],
        "swap10" => vec![Swap(ST10)],
        "swap11" => vec![Swap(ST11)],
        "swap12" => vec![Swap(ST12)],
        "swap13" => vec![Swap(ST13)],
        "swap14" => vec![Swap(ST14)],
        "swap15" => vec![Swap(ST15)],

        // Control flow
        "nop" => vec![Nop],
        "skiz" => vec![Skiz],
        "call" => vec![Call(parse_label(tokens)?)],
        "return" => vec![Return],
        "recurse" => vec![Recurse],
        "assert" => vec![Assert],
        "halt" => vec![Halt],

        // Memory access
        "read_mem" => vec![ReadMem],
        "write_mem" => vec![WriteMem],

        // Hashing-related instructions
        "hash" => vec![Hash],
        "divine_sibling" => vec![DivineSibling],
        "assert_vector" => vec![AssertVector],

        // Arithmetic on stack instructions
        "add" => vec![Add],
        "mul" => vec![Mul],
        "invert" => vec![Invert],
        "split" => vec![Split],
        "eq" => vec![Eq],
        "lsb" => vec![Lsb],
        "xxadd" => vec![XxAdd],
        "xxmul" => vec![XxMul],
        "xinvert" => vec![XInvert],
        "xbmul" => vec![XbMul],

        // Pseudo-instructions
        "neg" => vec![Push(BFieldElement::one().neg()), Mul],
        "sub" => vec![Swap(ST1), Push(BFieldElement::one().neg()), Mul, Add],

        "lte" => pseudo_instruction_lte(),
        "lt" => pseudo_instruction_lt(),
        "and" => pseudo_instruction_and(),
        "xor" => pseudo_instruction_xor(),
        "reverse" => pseudo_instruction_reverse(),
        "div" => pseudo_instruction_div(),

        "is_u32" => pseudo_instruction_is_u32(),
        "split_assert" => pseudo_instruction_split_assert(),

        "eq_vector" => pseudo_instruction_eq_vector(),

        // Read/write
        "read_io" => vec![ReadIo],
        "write_io" => vec![WriteIo],

        _ => return Err(anyhow::Error::new(UnknownInstruction(token.to_string()))),
    };

    let labelled_instruction = instruction
        .into_iter()
        .map(LabelledInstruction::Instruction)
        .collect();

    Ok(labelled_instruction)
}

fn pseudo_instruction_is_u32() -> Vec<AnInstruction<String>> {
    // _ a
    let mut instructions = vec![Dup(ST0)];
    // _ a a
    for _ in 0..32 {
        instructions.push(Lsb);
        // _ a (a>>i) b
        instructions.push(Pop);
        // _ a (a>>i)
    }
    instructions.push(Push(0_u64.into()));
    // _ a (a>>32) 0
    instructions.push(Eq);
    // _ a (a>>32)==0
    instructions.push(Assert);
    // _ a
    instructions
}

fn pseudo_instruction_split_assert() -> Vec<AnInstruction<String>> {
    vec![
        vec![Split],
        pseudo_instruction_is_u32(),
        vec![Swap(ST1)],
        pseudo_instruction_is_u32(),
        vec![Swap(ST1)],
    ]
    .concat()
}

fn pseudo_instruction_lte() -> Vec<AnInstruction<String>> {
    vec![
        vec![Push(-BFieldElement::new(1)), Mul, Add],
        pseudo_instruction_split_assert(),
        vec![Push(0_u64.into()), Eq, Swap(ST1), Pop],
    ]
    .concat()
}

fn pseudo_instruction_lt() -> Vec<AnInstruction<String>> {
    vec![vec![Push(1_u64.into()), Add], pseudo_instruction_lte()].concat()
}

fn pseudo_instruction_div() -> Vec<AnInstruction<String>> {
    vec![
        vec![
            // _ d n
            Divine(Some(Quotient)),
            // _ d n q
        ],
        pseudo_instruction_is_u32(),
        vec![
            // _ d n q
            Dup(ST2),
            // _ d n q d
            Dup(ST1),
            // _ d n q d q
            Mul,
            // _ d n q d·q
            Dup(ST2),
            // _ d n q d·q n
            Swap(ST1),
            // _ d n q n d·q
            Push(-BFieldElement::new(1)),
            // _ d n q n d·q -1
            Mul,
            // _ d n q n -d·q
            Add,
            // _ d n q r
            Dup(ST3),
            // _ d n q r d
            Dup(ST1),
            // _ d n q r d r
        ],
        pseudo_instruction_lt(),
        vec![
            // _ d n q r r<d
            Assert,
            // _ d n q r
            Swap(ST2),
            // _ d r q n
            Pop,
            // _ d r q
            Swap(ST2),
            // _ q r d
            Pop,
            // _ q r
        ],
    ]
    .concat()
}

fn pseudo_instruction_and() -> Vec<AnInstruction<String>> {
    let mut instructions = vec![];

    // decompose into bits, interleaved
    for _ in 0..32 {
        // _ A||a B||b
        instructions.push(Lsb);
        // _ A||a B b
        instructions.push(Swap(ST2));
        // _ b B A||a
        instructions.push(Lsb);
        // _ b B A a
        instructions.push(Swap(ST2));
        // _ b a A B
    }

    // assert u32-ness of A & B
    instructions.push(Push(0_u64.into()));
    instructions.push(Eq);
    instructions.push(Assert);
    // _ (b a)^32 A
    instructions.push(Push(0_u64.into()));
    instructions.push(Eq);
    instructions.push(Assert);
    // _ (b a)^32

    // start accumulating
    instructions.push(Push(0_u64.into()));

    for i in (0..32).rev() {
        // _ (b a)^i b a acc
        instructions.push(Swap(ST2));
        // _ (b a)^i acc a b
        instructions.push(Mul);
        // _ (b a)^i acc a&b
        instructions.push(Push((1_u64 << i).into()));
        // _ (b a)^i acc (a&b) 2^i
        instructions.push(Mul);
        // _ (b a)^i acc (a&b)·2^i
        instructions.push(Add);
        // _ (b a)^i acc'
    }

    instructions
}

fn pseudo_instruction_xor() -> Vec<AnInstruction<String>> {
    // a+b = a^b + (a&b)<<1 => a^b = a+b - 2·(a&b)
    // Credit: Daniel Lubarov
    vec![
        vec![Dup(ST1), Dup(ST1)],
        pseudo_instruction_and(),
        vec![Push(-BFieldElement::new(2)), Mul, Add, Add],
    ]
    .concat()
}

fn pseudo_instruction_reverse() -> Vec<AnInstruction<String>> {
    let mut instructions = vec![];

    // decompose into bits
    for _ in 0..32 {
        instructions.push(Lsb);
        instructions.push(Swap(ST1));
    }
    instructions.push(Push(0_u64.into()));
    instructions.push(Eq);
    instructions.push(Assert);

    // start accumulating
    instructions.push(Push(0_u64.into()));
    for i in 0..32 {
        instructions.push(Swap(ST1));
        instructions.push(Push((1_u64 << i).into()));
        instructions.push(Mul);
        instructions.push(Add);
    }
    instructions
}

fn pseudo_instruction_eq_vector() -> Vec<AnInstruction<String>> {
    vec![
        Dup(ST0),
        Dup(ST5),
        Eq,
        Dup(ST1),
        Dup(ST6),
        Eq,
        Mul,
        Dup(ST2),
        Dup(ST7),
        Eq,
        Mul,
        Dup(ST3),
        Dup(ST8),
        Eq,
        Mul,
        Dup(ST4),
        Dup(ST9),
        Eq,
        Mul,
    ]
}

fn parse_elem(tokens: &mut SplitWhitespace) -> Result<BFieldElement> {
    let constant_s = tokens.next().ok_or(UnexpectedEndOfStream)?;

    let mut constant_n128: i128 = constant_s.parse::<i128>()?;
    if constant_n128 < 0 {
        constant_n128 += BFieldElement::QUOTIENT as i128;
    }
    let constant_n64: u64 = constant_n128.try_into()?;
    let constant_elem = BFieldElement::new(constant_n64);

    Ok(constant_elem)
}

fn parse_label(tokens: &mut SplitWhitespace) -> Result<String> {
    let label = tokens
        .next()
        .map(|s| s.to_string())
        .ok_or(UnexpectedEndOfStream)?;

    Ok(label)
}

pub fn all_instructions_without_args() -> Vec<Instruction> {
    let all_instructions = vec![
        Pop,
        Push(Default::default()),
        Divine(None),
        Dup(Default::default()),
        Swap(Default::default()),
        Nop,
        Skiz,
        Call(Default::default()),
        Return,
        Recurse,
        Assert,
        Halt,
        ReadMem,
        WriteMem,
        Hash,
        DivineSibling,
        AssertVector,
        Add,
        Mul,
        Invert,
        Split,
        Eq,
        Lsb,
        XxAdd,
        XxMul,
        XInvert,
        XbMul,
        ReadIo,
        WriteIo,
    ];
    assert_eq!(Instruction::COUNT, all_instructions.len());
    all_instructions
}

pub fn all_labelled_instructions_with_args() -> Vec<LabelledInstruction> {
    vec![
        Pop,
        Push(BFieldElement::new(42)),
        Divine(None),
        Divine(Some(Quotient)),
        Dup(ST0),
        Dup(ST1),
        Dup(ST2),
        Dup(ST3),
        Dup(ST4),
        Dup(ST5),
        Dup(ST6),
        Dup(ST7),
        Dup(ST8),
        Dup(ST9),
        Dup(ST10),
        Dup(ST11),
        Dup(ST12),
        Dup(ST13),
        Dup(ST14),
        Dup(ST15),
        Swap(ST1),
        Swap(ST2),
        Swap(ST3),
        Swap(ST4),
        Swap(ST5),
        Swap(ST6),
        Swap(ST7),
        Swap(ST8),
        Swap(ST9),
        Swap(ST10),
        Swap(ST11),
        Swap(ST12),
        Swap(ST13),
        Swap(ST14),
        Swap(ST15),
        Nop,
        Skiz,
        Call("foo".to_string()),
        Return,
        Recurse,
        Assert,
        Halt,
        ReadMem,
        WriteMem,
        Hash,
        DivineSibling,
        AssertVector,
        Add,
        Mul,
        Invert,
        Split,
        Eq,
        Lsb,
        XxAdd,
        XxMul,
        XInvert,
        XbMul,
        ReadIo,
        WriteIo,
    ]
    .into_iter()
    .map(LabelledInstruction::Instruction)
    .collect()
}

pub mod sample_programs {
    pub const PUSH_PUSH_ADD_POP_S: &str = "
        push 1
        push 1
        add
        pop
    ";

    pub const EDGY_RAM_WRITES: &str = concat!();

    pub const READ_WRITE_X3: &str = "
        read_io
        read_io
        read_io
        write_io
        write_io
        write_io
    ";

    pub const READ_X3_WRITE_X14: &str = "
        read_io read_io read_io
        dup0 dup2 dup4
        dup0 dup2 dup4
        dup0 dup2 dup4
        dup0 dup2
        write_io write_io write_io write_io
        write_io write_io write_io write_io
        write_io write_io write_io write_io
        write_io write_io
    ";

    pub const HASH_HASH_HASH_HALT: &str = "
        hash
        hash
        hash
        halt
    ";

    pub const ALL_INSTRUCTIONS: &str = "
        pop
        push 42
        divine divine_quotient

        dup0 dup1 dup2 dup3 dup4 dup5 dup6 dup7 dup8 dup9 dup10 dup11 dup12 dup13 dup14 dup15
        swap1 swap2 swap3 swap4 swap5 swap6 swap7 swap8 swap9 swap10 swap11 swap12 swap13 swap14 swap15

        nop
        skiz
        call foo

        return recurse assert halt read_mem write_mem hash divine_sibling assert_vector
        add mul invert split eq lsb xxadd xxmul xinvert xbmul

        read_io write_io
    ";

    pub fn all_instructions_displayed() -> Vec<String> {
        vec![
            "pop",
            "push 42",
            "divine",
            "divine_quotient",
            "dup0",
            "dup1",
            "dup2",
            "dup3",
            "dup4",
            "dup5",
            "dup6",
            "dup7",
            "dup8",
            "dup9",
            "dup10",
            "dup11",
            "dup12",
            "dup13",
            "dup14",
            "dup15",
            "swap1",
            "swap2",
            "swap3",
            "swap4",
            "swap5",
            "swap6",
            "swap7",
            "swap8",
            "swap9",
            "swap10",
            "swap11",
            "swap12",
            "swap13",
            "swap14",
            "swap15",
            "nop",
            "skiz",
            "call foo",
            "return",
            "recurse",
            "assert",
            "halt",
            "read_mem",
            "write_mem",
            "hash",
            "divine_sibling",
            "assert_vector",
            "add",
            "mul",
            "invert",
            "split",
            "eq",
            "lsb",
            "xxadd",
            "xxmul",
            "xinvert",
            "xbmul",
            "read_io",
            "write_io",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }
}

#[cfg(test)]
mod instruction_tests {
    use itertools::Itertools;
    use num_traits::One;
    use num_traits::Zero;
    use strum::EnumCount;
    use strum::IntoEnumIterator;
    use twenty_first::shared_math::b_field_element::BFieldElement;

    use crate::instruction::all_labelled_instructions_with_args;
    use crate::ord_n::Ord7;
    use crate::program::Program;

    use super::all_instructions_without_args;
    use super::parse;
    use super::sample_programs;
    use super::AnInstruction::{self, *};

    #[test]
    fn opcode_test() {
        // test for duplicates
        let mut opcodes = vec![];
        for instruction in AnInstruction::<BFieldElement>::iter() {
            assert!(
                !opcodes.contains(&instruction.opcode()),
                "Have different instructions with same opcode."
            );
            opcodes.push(instruction.opcode());
        }

        for opc in opcodes.iter() {
            println!(
                "opcode {} exists: {}",
                opc,
                AnInstruction::<BFieldElement>::try_from(*opc).unwrap()
            );
        }

        // assert size of list corresponds to number of opcodes
        assert_eq!(
            AnInstruction::<BFieldElement>::COUNT,
            opcodes.len(),
            "Mismatch in number of instructions!"
        );

        // assert iter method also covers push
        assert!(
            opcodes.contains(&AnInstruction::<BFieldElement>::Push(Default::default()).opcode()),
            "list of opcodes needs to contain push"
        );

        // test for width
        let max_opcode: u32 = AnInstruction::<BFieldElement>::iter()
            .map(|inst| inst.opcode())
            .max()
            .unwrap();
        let mut num_bits = 0;
        while (1 << num_bits) < max_opcode {
            num_bits += 1;
        }
        assert!(
            num_bits <= Ord7::COUNT,
            "Biggest instruction needs more than {} bits :(",
            Ord7::COUNT
        );

        // assert consistency
        for instruction in AnInstruction::<BFieldElement>::iter() {
            assert!(
                instruction == instruction.opcode().try_into().unwrap(),
                "instruction to opcode map must be consistent"
            );
        }
    }

    #[test]
    fn parse_push_pop_test() {
        let code = "
            push 1
            push 1
            add
            pop
        ";
        let program = Program::from_code(code).unwrap();
        let instructions = program.into_iter().collect_vec();
        let expected = vec![
            Push(BFieldElement::one()),
            Push(BFieldElement::one()),
            Add,
            Pop,
        ];

        assert_eq!(expected, instructions);
    }

    #[test]
    fn parse_and_display_each_instruction_test() {
        let expected = all_labelled_instructions_with_args();
        let actual = parse(sample_programs::ALL_INSTRUCTIONS).unwrap();

        assert_eq!(expected, actual);

        for (actual, expected) in actual
            .iter()
            .map(|instr| format!("{}", instr))
            .zip_eq(sample_programs::all_instructions_displayed())
        {
            assert_eq!(expected, actual);
        }
    }

    #[test]
    fn fail_on_duplicate_labels_test() {
        let code = "
            push 2
            call foo
            bar: push 2
            foo: push 3
            foo: push 4
            halt
        ";
        let program = Program::from_code(code);
        assert!(
            program.is_err(),
            "Duplicate labels should result in a parse error"
        );
    }

    #[test]
    fn ib_registers_are_binary_test() {
        use Ord7::*;

        for instruction in all_instructions_without_args() {
            for ib in [IB0, IB1, IB2, IB3, IB4, IB5, IB6] {
                let ib_value = instruction.ib(ib);
                assert!(
                    ib_value.is_zero() || ib_value.is_one(),
                    "IB{} for instruction {} is 0 or 1 ({})",
                    ib,
                    instruction,
                    ib_value
                );
            }
        }
    }

    #[test]
    fn instruction_to_opcode_to_instruction_is_consistent_test() {
        for instr in all_instructions_without_args() {
            assert_eq!(instr, instr.opcode().try_into().unwrap());
        }
    }

    #[test]
    fn print_all_instructions_and_opcodes() {
        for instr in all_instructions_without_args() {
            println!("{:>3} {: <10}", instr.opcode(), format!("{instr}"));
        }
    }
}
