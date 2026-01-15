const path = require('path');

// Load the native addon directly
const nativeBinding = require(path.join(__dirname, 'mtga_reader_node.node'));

// Utility functions
module.exports.isAdmin = nativeBinding.isAdmin
module.exports.findProcess = nativeBinding.findProcess
module.exports.init = nativeBinding.init
module.exports.close = nativeBinding.close
module.exports.isInitialized = nativeBinding.isInitialized

// Assembly functions
module.exports.getAssemblies = nativeBinding.getAssemblies
module.exports.getAssemblyClasses = nativeBinding.getAssemblyClasses
module.exports.getClassDetails = nativeBinding.getClassDetails

// Instance reading functions
module.exports.getInstance = nativeBinding.getInstance
module.exports.getInstanceField = nativeBinding.getInstanceField
module.exports.getStaticField = nativeBinding.getStaticField

// Dictionary reading
module.exports.getDictionary = nativeBinding.getDictionary

// High-level data reading
module.exports.readData = nativeBinding.readData
module.exports.readClass = nativeBinding.readClass
module.exports.readGenericInstance = nativeBinding.readGenericInstance
