Feature: Bullet ballistics

  Background:
    Given I am in test room 1
    And I have the smg
    And the target dummy is out of the way

  Scenario: A bullet leaves at the Sterling's muzzle velocity
    When I fire once
    Then the bullet leaves at between 340 and 372 metres per second

  Scenario: Air drag slows the bullet down
    Given I press Z
    And I am prone
    And I right click
    And the gun is on its sights
    When I fire once
    Then the bullet has slowed to between 250 and 335 metres per second after 100 metres

  Scenario: Gravity pulls the bullet down
    Given I right click
    And the gun is on its sights
    When I fire once
    Then the bullet is falling after 150 metres

  Scenario: A wind from the west blows the bullet east
    Given the wind blows east at 20 metres per second
    And I press Z
    And I am prone
    And I right click
    And the gun is on its sights
    When I fire once
    Then the bullet is being blown east after 100 metres

  Scenario: A wind from the east blows the bullet west
    Given the wind blows west at 20 metres per second
    And I press Z
    And I am prone
    And I right click
    And the gun is on its sights
    When I fire once
    Then the bullet is being blown west after 100 metres

  Scenario: In still air the bullet is not blown sideways
    Given the wind blows east at 0 metres per second
    And I press Z
    And I am prone
    And I right click
    And the gun is on its sights
    When I fire once
    Then the bullet is not being blown sideways after 100 metres
