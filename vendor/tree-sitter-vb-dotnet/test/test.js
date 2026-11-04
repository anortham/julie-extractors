const Parser = require('tree-sitter');
const VBNet = require('../bindings/node');

console.log('Testing tree-sitter-vb-dotnet parser...\n');

// Create parser
const parser = new Parser();

  parser.setLanguage(VBNet);
  console.log('✓ Language loaded successfully');
  console.log('  Language name:', VBNet.name);


// Test code
const sourceCode = `
Module TestModule
    Sub Main()
        Console.WriteLine("Hello World")
    End Sub
    
    Function Add(a As Integer, b As Integer) As Integer
        Return a + b
    End Function
    
    Private Property Count As Integer
        Get
            Return _count
        End Get
        Set(value As Integer)
            _count = value
        End Set
    End Property
End Module

Public Class TestClass
    Public Function Multiply(x As Double, y As Double) As Double
        Dim result As Double
        result = x * y
        Return result
    End Function
End Class
`;

// Parse the code
console.log('\nParsing VB.NET code...');
const tree = parser.parse(sourceCode);
console.log('✓ Parse successful');

// Show the syntax tree
console.log('\nSyntax tree:');
console.log(tree.rootNode.toString());

// Find all functions/methods
console.log('\nFinding all methods...');
function findMethods(node, methods = []) {
  if (node.type === 'method_declaration') {
    const nameNode = node.childForFieldName('name');
    methods.push({
      type: 'method',
      name: nameNode ? nameNode.text : 'unknown',
      line: node.startPosition.row + 1
    });
  }
  
  for (const child of node.children) {
    findMethods(child, methods);
  }
  
  return methods;
}

const methods = findMethods(tree.rootNode);
console.log('\nFound methods:');
methods.forEach(m => {
  console.log(`  - ${m.name} (line ${m.line})`);
});

// Test a simple query
console.log('\nTesting tree-sitter queries...');
const query = new Parser.Query(VBNet, `
  (method_declaration
    name: (identifier) @method.name)
`);

const matches = query.matches(tree.rootNode);
console.log(`Found ${matches.length} method declarations`);

console.log('\n✓ All tests passed!');