/**
 * Test file for mtga-reader-node native addon
 *
 * Prerequisites:
 * 1. Run as Administrator
 * 2. MTGA must be running
 *
 * Usage:
 *   npm run build
 *   npm test
 */

const mtga = require('./index');

console.log('='.repeat(60));
console.log('MTGA Reader Node.js Test');
console.log('='.repeat(60));

// Check admin privileges
console.log('\n[1] Checking admin privileges...');
const isAdmin = mtga.isAdmin();
console.log(`    Admin: ${isAdmin}`);

if (!isAdmin) {
    console.error('\n❌ Error: This program requires administrator privileges.');
    console.error('   Please run as Administrator.');
    process.exit(1);
}

// Check if MTGA is running
console.log('\n[2] Looking for MTGA process...');
const processFound = mtga.findProcess('MTGA');
console.log(`    Process found: ${processFound}`);

if (!processFound) {
    console.error('\n❌ Error: MTGA process not found.');
    console.error('   Please start MTGA first.');
    process.exit(1);
}

// Initialize the reader
console.log('\n[3] Initializing reader...');
try {
    mtga.init('MTGA');
    console.log('    Initialized: true');
} catch (err) {
    console.error(`\n❌ Error initializing: ${err.message}`);
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

// Get classes from Assembly-CSharp
console.log('\n[5] Getting classes from Assembly-CSharp...');
try {
    const classes = mtga.getAssemblyClasses('Assembly-CSharp');
    console.log(`    Found ${classes.length} classes`);

    // Find WrapperController
    const wrapperController = classes.find(c => c.name === 'WrapperController');
    if (wrapperController) {
        console.log(`\n    Found WrapperController at address: 0x${wrapperController.address.toString(16)}`);
    }
} catch (err) {
    console.error(`    Error: ${err.message}`);
}

// Get class details for WrapperController
console.log('\n[6] Getting WrapperController details...');
try {
    const details = mtga.getClassDetails('Assembly-CSharp', 'WrapperController');
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

// Test high-level read_data function - get dictionary address
console.log('\n[7] Testing readData (reading card collection path)...');
let cardsAddress = null;
try {
    const path = [
        'WrapperController',
        '<Instance>k__BackingField',
        '<InventoryManager>k__BackingField',
        '_inventoryServiceWrapper',
        '<Cards>k__BackingField',
    ];
    const data = mtga.readData('MTGA', path);
    console.log('    Result type:', typeof data);
    if (data && data.error) {
        console.log('    Error:', data.error);
    } else if (data) {
        console.log('    Dictionary info:');
        console.log(`      _count: ${data._count}`);
        console.log(`      _version: ${data._version}`);
        // The _entries field contains a pointer to the entries array
        if (data._entries && data._entries.address) {
            cardsAddress = data._entries.address;
            console.log(`      _entries address: 0x${cardsAddress.toString(16)}`);
        }
    }
} catch (err) {
    console.error(`    Error: ${err.message}`);
}

// Test reading the dictionary entries using getDictionary
console.log('\n[8] Testing readData with _entries (full card path)...');
try {
    const path = [
        'WrapperController',
        '<Instance>k__BackingField',
        '<InventoryManager>k__BackingField',
        '_inventoryServiceWrapper',
        '<Cards>k__BackingField',
        '_entries',
    ];
    const data = mtga.readData('MTGA', path);
    console.log('    Result type:', typeof data);
    if (data && data.error) {
        console.log('    Error:', data.error);
    } else if (Array.isArray(data)) {
        console.log(`    Got ${data.length} card entries`);
        if (data.length > 0) {
            console.log('    First 3 entries:');
            data.slice(0, 3).forEach((entry, i) => {
                console.log(`      [${i}] key: ${entry.key}, value: ${entry.value}`);
            });
        }
    } else if (data) {
        console.log('    Data received (truncated):', JSON.stringify(data).slice(0, 300) + '...');
    }
} catch (err) {
    console.error(`    Error: ${err.message}`);
}

// Cleanup
console.log('\n[9] Closing reader...');
try {
    mtga.close();
    console.log('    Closed: true');
} catch (err) {
    console.error(`    Error: ${err.message}`);
}

console.log('\n' + '='.repeat(60));
console.log('Test completed!');
console.log('='.repeat(60));
