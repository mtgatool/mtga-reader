/**
 * Test file for mtga-reader package
 *
 * Prerequisites:
 * 1. Run as Administrator (Windows) or with sudo (macOS)
 * 2. MTGA must be running
 *
 * Usage:
 *   npm install
 *   npm test
 */

const mtga = require('mtga-reader');

const isMacOS = process.platform === 'darwin';

console.log('='.repeat(60));
console.log('MTGA Reader Node.js Test');
console.log(`Platform: ${process.platform}`);
console.log('='.repeat(60));

// Check admin privileges
console.log('\n[1] Checking admin privileges...');
const isAdmin = mtga.isAdmin();
console.log(`    Admin: ${isAdmin}`);

if (!isAdmin) {
    console.error('\n Error: This program requires administrator privileges.');
    console.error('   Windows: Run as Administrator');
    console.error('   macOS: Run with sudo');
    process.exit(1);
}

// Check if MTGA is running
console.log('\n[2] Looking for MTGA process...');
const processFound = mtga.findProcess('MTGA');
console.log(`    Process found: ${processFound}`);

if (!processFound) {
    console.error('\n Error: MTGA process not found.');
    console.error('   Please start MTGA first.');
    process.exit(1);
}

// Initialize the reader
console.log('\n[3] Initializing reader...');
try {
    mtga.init('MTGA');
    console.log('    Initialized: true');
} catch (err) {
    console.error(`\n Error initializing: ${err.message}`);
    process.exit(1);
}

// Get assemblies
console.log('\n[4] Getting assemblies...');
try {
    const assemblies = mtga.getAssemblies();
    console.log(`    Found ${assemblies.length} assemblies:`);
    assemblies.slice(0, 10).forEach(a => console.log(`      - ${a}`));
    if (assemblies.length > 10) {
        console.log(`      ... and ${assemblies.length - 10} more`);
    }
} catch (err) {
    console.error(`    Error: ${err.message}`);
}

// Get classes from assembly
console.log('\n[5] Getting classes...');
try {
    const assemblyName = isMacOS ? 'GameAssembly' : 'Assembly-CSharp';
    const classes = mtga.getAssemblyClasses(assemblyName);
    console.log(`    Found ${classes.length} classes`);

    // Find key classes
    const targetClass = isMacOS ? 'PAPA' : 'WrapperController';
    const found = classes.find(c => c.name === targetClass);
    if (found) {
        console.log(`\n    Found ${targetClass} at address: 0x${found.address.toString(16)}`);
    }
} catch (err) {
    console.error(`    Error: ${err.message}`);
}

// Get class details
console.log('\n[6] Getting class details...');
try {
    const assemblyName = isMacOS ? 'GameAssembly' : 'Assembly-CSharp';
    const className = isMacOS ? 'PAPA' : 'WrapperController';
    const details = mtga.getClassDetails(assemblyName, className);
    console.log(`    Class: ${details.namespace}.${details.name}`);
    console.log(`    Address: 0x${details.address.toString(16)}`);
    console.log(`    Fields: ${details.fields.length}`);

    // Show some fields
    details.fields.slice(0, 5).forEach(f => {
        console.log(`      - ${f.name}: ${f.typeName} (offset: ${f.offset})`);
    });

    // Show static instances
    if (details.staticInstances.length > 0) {
        console.log(`    Static instances:`);
        details.staticInstances.forEach(si => {
            console.log(`      - ${si.fieldName}: 0x${si.address.toString(16)}`);
        });
    }
} catch (err) {
    console.error(`    Error: ${err.message}`);
}

// Test high-level read_data function
console.log('\n[7] Testing readData (reading card collection)...');
try {
    // Different paths for different platforms
    // macOS: Uses backing field names from C# auto-properties
    // Windows: Uses similar pattern but may differ based on Mono reflection
    const path = isMacOS
        ? [
            'PAPA',
            '<InventoryManager>k__BackingField',
            '_inventoryServiceWrapper',
            '<Cards>k__BackingField',
        ]
        : [
            'WrapperController',
            '<Instance>k__BackingField',
            '<InventoryManager>k__BackingField',
            '_inventoryServiceWrapper',
            '<Cards>k__BackingField',
        ];

    console.log(`    Using path: ${path.join(' -> ')}`);
    const data = mtga.readData('MTGA', path);

    if (data && data.error) {
        console.log('    Error:', data.error);
    } else if (Array.isArray(data)) {
        console.log(`    Got ${data.length} card entries`);
        if (data.length > 0) {
            console.log('    First 5 entries:');
            data.slice(0, 5).forEach((entry, i) => {
                console.log(`      [${i}] card ID: ${entry.key}, quantity: ${entry.value}`);
            });
        }
    } else if (data) {
        console.log('    Data received:', JSON.stringify(data, null, 2).slice(0, 500));
    }
} catch (err) {
    console.error(`    Error: ${err.message}`);
}

// Cleanup
console.log('\n[8] Closing reader...');
try {
    mtga.close();
    console.log('    Closed: true');
} catch (err) {
    console.error(`    Error: ${err.message}`);
}

console.log('\n' + '='.repeat(60));
console.log('Test completed!');
console.log('='.repeat(60));
