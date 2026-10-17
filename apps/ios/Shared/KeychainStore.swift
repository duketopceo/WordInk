import Foundation
import Security

/// Generic-password Keychain item holding the device token (R7).
/// AfterFirstUnlock keeps it readable from background-launched intents.
enum KeychainStore {
    static let service = "com.duketopceo.wordink.gateway"
    private static let account = "device-token"

    private static var query: [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
    }

    static func loadToken() -> String? {
        var q = query
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &item) == errSecSuccess,
              let data = item as? Data,
              let token = String(data: data, encoding: .utf8) else { return nil }
        return token
    }

    /// OSStatus-returning variant so callers/tests can see the real failure
    /// (e.g. errSecMissingEntitlement) instead of a bare Bool.
    @discardableResult
    static func saveTokenStatus(_ token: String) -> OSStatus {
        let data = Data(token.utf8)
        var attrs = query
        attrs[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        attrs[kSecValueData as String] = data
        let status = SecItemAdd(attrs as CFDictionary, nil)
        if status == errSecDuplicateItem {
            return SecItemUpdate(query as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        }
        return status
    }

    @discardableResult
    static func saveToken(_ token: String) -> Bool {
        saveTokenStatus(token) == errSecSuccess
    }

    @discardableResult
    static func deleteToken() -> Bool {
        let status = SecItemDelete(query as CFDictionary)
        return status == errSecSuccess || status == errSecItemNotFound
    }
}
