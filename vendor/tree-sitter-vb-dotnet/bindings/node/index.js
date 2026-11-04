// const root = require("path").join(__dirname, "..", "..");


// module.exports =
//   typeof process.versions.bun === "string"
//     // Support `bun build --compile` by being statically analyzable enough to find the .node file at build-time
//     ? require(`../../prebuilds/${process.platform}-${process.arch}/tree-sitter-vb-dotnet.node`)
//     : require("node-gyp-build")(root);

// try {
//   module.exports.nodeTypeInfo = require("../../src/node-types.json");
// } catch (_) {}
const root = require("path").join(__dirname, "..", "..");

let binding;
try {
  binding = typeof process.versions.bun === "string"
    ? require(`../../prebuilds/${process.platform}-${process.arch}/tree-sitter-vb-dotnet.node`)
    : require("node-gyp-build")(root);
} catch (error) {
  // Fallback to direct binding
  binding = require("../../build/Release/tree_sitter_vb_dotnet_binding.node");
}

if (!binding.language) {
  throw new Error("Language not found in binding");
}

// We need to export the raw language for tree-sitter to accept it,
// but then monkey-patch the properties after it's been accepted
const language = binding.language;

// Add basic properties
try {
  language.name = "vb_dotnet";
  language.nodeTypeInfo = require("../../src/node-types.json");
} catch (_) {
  console.error("Warning: Could not load node-types.json");
}

// We need to patch tree-sitter's internals to provide the nodeSubclasses
// Since we can't modify the N-API External object, we'll patch the Tree prototype
const Parser = require('tree-sitter');
const TreeSitterModule = require('tree-sitter');

// Get required components
let runtimeBinding, nodeTypeNamesById, nodeFieldNamesById, nodeSubclasses;

try {
  runtimeBinding = require('tree-sitter/build/Release/tree_sitter_runtime_binding.node') ||
                   require(`tree-sitter/prebuilds/${process.platform}-${process.arch}/tree-sitter.node`);
  
  nodeTypeNamesById = runtimeBinding.getNodeTypeNamesById(language);
  nodeFieldNamesById = runtimeBinding.getNodeFieldNamesById(language);
  const {SyntaxNode} = TreeSitterModule;
  
  nodeSubclasses = [];
  for (let id = 0, n = nodeTypeNamesById.length; id < n; id++) {
    nodeSubclasses[id] = SyntaxNode;
  }
  
  console.log('Initialized nodeSubclasses with', nodeSubclasses.length, 'node types');
} catch (error) {
  console.error('Failed to initialize nodeSubclasses:', error.message);
}

// Patch Parser.prototype.getLanguage to return our enhanced language object
const originalGetLanguage = Parser.prototype.getLanguage;

if (originalGetLanguage && nodeSubclasses) {
  Parser.prototype.getLanguage = function() {
    const originalLanguage = originalGetLanguage.call(this);
    
    // If this is our language, return a proxy with the required properties
    if (originalLanguage === language) {
      return new Proxy(originalLanguage, {
        get(target, prop) {
          if (prop === 'nodeSubclasses') return nodeSubclasses;
          if (prop === 'nodeTypeNamesById') return nodeTypeNamesById;
          if (prop === 'nodeFieldNamesById') return nodeFieldNamesById;
          return target[prop];
        }
      });
    }
    
    return originalLanguage;
  };
  
  console.log('Patched Parser.prototype.getLanguage for VB.NET');
}

module.exports = language;

