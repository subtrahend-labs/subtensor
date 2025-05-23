export const IMETAGRAPH_ADDRESS = "0x0000000000000000000000000000000000000802";

export const IMetagraphABI = [
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
            {
                internalType: "uint16",
                name: "uid",
                type: "uint16",
            },
        ],
        name: "getAxon",
        outputs: [
            {
                components: [
                    {
                        internalType: "uint64",
                        name: "block",
                        type: "uint64",
                    },
                    {
                        internalType: "uint32",
                        name: "version",
                        type: "uint32",
                    },
                    {
                        internalType: "uint128",
                        name: "ip",
                        type: "uint128",
                    },
                    {
                        internalType: "uint16",
                        name: "port",
                        type: "uint16",
                    },
                    {
                        internalType: "uint8",
                        name: "ip_type",
                        type: "uint8",
                    },
                    {
                        internalType: "uint8",
                        name: "protocol",
                        type: "uint8",
                    },
                ],
                internalType: "struct AxonInfo",
                name: "",
                type: "tuple",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
            {
                internalType: "uint16",
                name: "uid",
                type: "uint16",
            },
        ],
        name: "getColdkey",
        outputs: [
            {
                internalType: "bytes32",
                name: "",
                type: "bytes32",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
            {
                internalType: "uint16",
                name: "uid",
                type: "uint16",
            },
        ],
        name: "getConsensus",
        outputs: [
            {
                internalType: "uint16",
                name: "",
                type: "uint16",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
            {
                internalType: "uint16",
                name: "uid",
                type: "uint16",
            },
        ],
        name: "getDividends",
        outputs: [
            {
                internalType: "uint16",
                name: "",
                type: "uint16",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
            {
                internalType: "uint16",
                name: "uid",
                type: "uint16",
            },
        ],
        name: "getEmission",
        outputs: [
            {
                internalType: "uint64",
                name: "",
                type: "uint64",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
            {
                internalType: "uint16",
                name: "uid",
                type: "uint16",
            },
        ],
        name: "getHotkey",
        outputs: [
            {
                internalType: "bytes32",
                name: "",
                type: "bytes32",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
            {
                internalType: "uint16",
                name: "uid",
                type: "uint16",
            },
        ],
        name: "getIncentive",
        outputs: [
            {
                internalType: "uint16",
                name: "",
                type: "uint16",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
            {
                internalType: "uint16",
                name: "uid",
                type: "uint16",
            },
        ],
        name: "getIsActive",
        outputs: [
            {
                internalType: "bool",
                name: "",
                type: "bool",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
            {
                internalType: "uint16",
                name: "uid",
                type: "uint16",
            },
        ],
        name: "getLastUpdate",
        outputs: [
            {
                internalType: "uint64",
                name: "",
                type: "uint64",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
            {
                internalType: "uint16",
                name: "uid",
                type: "uint16",
            },
        ],
        name: "getRank",
        outputs: [
            {
                internalType: "uint16",
                name: "",
                type: "uint16",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
            {
                internalType: "uint16",
                name: "uid",
                type: "uint16",
            },
        ],
        name: "getStake",
        outputs: [
            {
                internalType: "uint64",
                name: "",
                type: "uint64",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
            {
                internalType: "uint16",
                name: "uid",
                type: "uint16",
            },
        ],
        name: "getTrust",
        outputs: [
            {
                internalType: "uint16",
                name: "",
                type: "uint16",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
        ],
        name: "getUidCount",
        outputs: [
            {
                internalType: "uint16",
                name: "",
                type: "uint16",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
            {
                internalType: "uint16",
                name: "uid",
                type: "uint16",
            },
        ],
        name: "getValidatorStatus",
        outputs: [
            {
                internalType: "bool",
                name: "",
                type: "bool",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
            {
                internalType: "uint16",
                name: "uid",
                type: "uint16",
            },
        ],
        name: "getVtrust",
        outputs: [
            {
                internalType: "uint16",
                name: "",
                type: "uint16",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
];